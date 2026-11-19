#!/bin/bash

QLOGS_DIR="./out_json"

# pull the client qlogs
rsync -avz vm-geant:/root/out/qlogs/uploaded-qlogs/ ${QLOGS_DIR}

# pull the server's qlog
rsync -avz vm-geant:/root/out/source-fcping/qlogs/ ${QLOGS_DIR}