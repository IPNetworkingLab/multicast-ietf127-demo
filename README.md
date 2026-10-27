# multicast-ietf127-demo

## FCQUIC-Beacon

### Binaries
The two following binaries are implemented as standalone applications that do not depend on the FCQUIC implementation. Rather, the source writes files to disk, then sends the filename over a UnixDatagram socket to the FCQUIC source binary, which then transmits the file using multicast to the FCQUIC receiver, which will write it to disk, and send the filename to the receiver application over a UnixDatagram socket.
- `source.rs` 
- `receiver.rs`

The following binaries are applications built using `tokio-fcquiche`, thereby depending on the full FCQUIC applications. In the source binary, the beacon creation and transmission is done in a tokio task, and transmission over multicast,... is handled by `tokio-fcquiche`, which exposes a channel to send information over multicast (and/or unicast), and another channel to receive information from the clients,...
The receiver binary is identical in its architecture.
- `tfc-source.rs`
- `tfc-receiver.rs`
