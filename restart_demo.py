#!/usr/bin/env python3


import argparse
import fcntl
import os
import re
import signal
import subprocess
import sys
import threading
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

REPO = Path(__file__).resolve().parent
# remote workdir
REMOTE_DIR = "~"

# this var holds the lock file and the per-source .<name>.last-restart files (mtime = last restart)
STATE_DIR = REPO / "out"

# ntfy.sh topic name, used to send a phone notification in case of issues
NTFY_TOPIC = "fcquic-demo-618c4dfef77b5e52"
COOLDOWN = 10 * 60  # seconds
RECEIVER_DELAY = 2  # seconds between the source container start and the receivers
START_TIMEOUT = 15 * 60  # saving the previous logs can take a while


@dataclass
class Cmd:
    cmd: str
    cwd: str = str(REPO)
    host: str | None = None  # ssh host, set to None will run the command locally


@dataclass
class Check:
    # the source is considered healthy when "pattern" is found in the logs during the last "window" seconds

    container: str
    pattern: str

    # if the string alert_pattern was found in the logs, then send an alert but don't restart
    alert_pattern: str | None = None

    # window of time (in seconds) that the health check operates on
    # this is ideally the same value as the cron interval, so that the logs are only checked once
    window: int = 120
    host: str | None = None  # ssh host, None runs it locally


@dataclass
class Source:
    name: str
    start: Cmd
    # receivers must be run scripts that honour STOP_ONLY=1 (save logs, remove container, exit)
    receivers: list[Cmd] = field(default_factory=list)
    check: Check | None = None  # None: never restarted by the watchdog


# list of sources (and potentially receivers) to run and monitor
SOURCES = [
    Source(
        name="fcquic",
        start=Cmd(
            "./run_fcquic_beacon.sh --send-interval 4000 --payload-every 5 --payload-size 100000",
            cwd=REMOTE_DIR,
            host="vm-geant-cron",
        ),
        receivers=[Cmd("./run_docker_recv.sh")],
        check=Check(
            "fcquic-recv1", r"tfc_receiver\] Heartbeat #", alert_pattern=r"\bERROR\b"
        ),
    ),
    # Source(
    #     name="gpac",
    #     start=Cmd("./run_gpac_source.sh ...", cwd=REMOTE_DIR, host="vm-geant-cron"),
    # ),
]


def log(name: str, msg: str):
    print(f"{datetime.now():%Y-%m-%d %H:%M:%S} [{name}] {msg}", flush=True)


# send a notification in case of an issue, e.g., if no heartbeats were seen in the last 2 minutes and a restart was done
def notify(name: str, title: str, msg: str, priority: str = "default"):
    log(name, f"alert: {title}: {msg}")

    req = urllib.request.Request(
        f"https://ntfy.sh/{NTFY_TOPIC}",
        data=msg.encode(),
        headers={"Title": f"[{name}] {title}", "Priority": priority},
    )
    try:
        urllib.request.urlopen(req, timeout=10)
    except OSError as e:
        log(name, f"couldn't send an alert: {e}")


# spawn the command cmd, with an optional env
def spawn(cmd: Cmd, env: str = "", capture: bool = True) -> subprocess.Popen:
    # change dir to the cwd of the cmd, then run the commadn with the given env vars
    shell = f"cd {cmd.cwd} && {env}{cmd.cmd}"

    # if host is None then run locally
    if cmd.host is None:
        argv = ["bash", "-c", shell]
    else:
        # -tt makes it so that killing the ssh command will stop the session, which will also end any running command such as docker logs -f,...
        argv = ["ssh", "-tt", "-o", "BatchMode=yes", cmd.host, shell]
    return subprocess.Popen(
        argv,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
        text=True,
        errors="replace",
        start_new_session=True,
    )


def kill(proc: subprocess.Popen):
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    proc.wait()


# run the run script until the container starts, alert if it doesn't
def run_until_started(name: str, cmd: Cmd) -> bool:
    log(name, f"starting: {cmd.cmd}")
    proc = spawn(cmd)
    # kill the command if the container doesn't start
    timer = threading.Timer(START_TIMEOUT, kill, [proc])
    timer.start()

    started = False
    for line in proc.stdout:
        line = line.strip()
        log(name, line)

        if started:
            # if the container was started in the prev iter, we can exit
            break

        # check if the line contains the container id that was printed by docker, if so then the container was started
        started = re.fullmatch(r"[0-9a-f]{64}", line) is not None

    timer.cancel()
    # kill the ssh command, the container should keep running in detached mode
    kill(proc)

    if started:
        log(name, "started")
    else:
        # notify when we failed to start the container
        notify(name, "restart failed", f"{cmd.cmd} failed to start", priority="urgent")
    return started


def last_restart_file(src: Source) -> Path:
    return STATE_DIR / f".{src.name}.last-restart"


# to restart the beacon, we first need to tell the receivers to stop
# then restart the source container, this will make the source save it's logs
def restart(src: Source) -> bool:
    last_restart_file(src).touch()

    receivers_stopping = [
        spawn(receiver, env="STOP_ONLY=1 ", capture=False) for receiver in src.receivers
    ]

    if not run_until_started(src.name, src.start):
        return False

    for receiver_stop in receivers_stopping:
        receiver_stop.wait()

    time.sleep(RECEIVER_DELAY)

    # a list and not a generator so that all receivers are started even if one fails
    return all([run_until_started(src.name, receiver) for receiver in src.receivers])


# run the health check on the logs
def recent_logs(check: Check) -> list[str]:
    proc = spawn(
        Cmd(
            f"docker logs --since {check.window}s {check.container}",
            cwd="~",
            host=check.host,
        )
    )
    try:
        return proc.communicate(timeout=60)[0].splitlines()
    except subprocess.TimeoutExpired:
        kill(proc)
        return []


def watchdog(src: Source) -> bool:
    src_check = src.check
    lines = recent_logs(src_check)

    if src_check.alert_pattern and (
        errors := [l for l in lines if re.search(src_check.alert_pattern, l)]
    ):
        notify(
            src.name,
            f"{len(errors)} error(s) in {src_check.container}",
            "\n".join(errors[-5:]),
        )

    matches = sum(1 for l in lines if re.search(src_check.pattern, l))
    if matches:
        log(src.name, f"healthy: {matches} matches in the last {src_check.window}s")
        return True

    f = last_restart_file(src)
    if f.exists() and (since := time.time() - f.stat().st_mtime) < COOLDOWN:
        log(src.name, f"unhealthy, restarted {since:.0f}s ago, waiting")
        return False

    notify(
        src.name,
        "restarting",
        f"nothing from {src_check.container} in the last {src_check.window}s",
        priority="high",
    )
    return restart(src)


def main():
    parser = argparse.ArgumentParser(
        description="""\
this script is used to keep the multicast demo running, it restarts the sources and their receivers.
This is needed since fcquic sometimes stalls after a few hours, and it also stalls when no clients are connected

usage:
  ./restart_demo.py             restarts all the sources in SOURCES, then their receivers
  ./restart_demo.py --watchdog  only restarts the sources whose check fails (e.g., no heartbeat in the last 2 mins)
                                -> a source isn't restarted again within 10 mins of its last restart

alerts are sent with ntfy, subscribe to NTFY_TOPIC in the ntfy app to get them

crontab (watchdog on odd minutes so it doesn't take the lock from the periodic restart):
  0 */2 * * * cd /home/corentin/multicast-ietf127-demo && ./restart_demo.py >> out/restart_demo.log 2>&1
  1-59/2 * * * * cd /home/corentin/multicast-ietf127-demo && ./restart_demo.py --watchdog >> out/restart_demo.log 2>&1
""",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--watchdog", action="store_true", help="only restart the unhealthy sources"
    )
    args = parser.parse_args()

    # use a lock file to make sure that no other watchdog script is running
    STATE_DIR.mkdir(exist_ok=True)
    lock = open(STATE_DIR / ".restart_demo.lock", "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        log("main", "another restart_demo.py is running, exiting")
        return

    sources = (
        [source for source in SOURCES if source.check] if args.watchdog else SOURCES
    )
    with ThreadPoolExecutor() as pool:
        results = list(pool.map(watchdog if args.watchdog else restart, sources))
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
