#!/bin/bash

if [[ -z $1 ]];
then
    printf "💎 DIP-20 Deploy Script:\n\n   usage: deploy <local|ic|other> [reinstall]\n\n"
    exit 1;
fi
NETWORK=$1
MODE=$2

if [ -z $GENESIS_AMT ]; then
  GENESIS_AMT="1000000000"
fi

source scripts/cap_service.sh # this handles setting the cap id variable, and checks to see if it's already been set

if [[ "$MODE" == "reinstall" ]]; then
  MODE="--mode reinstall"
fi

dfx deploy --no-wallet --network $NETWORK token \
	--argument="(
        \"data:image/jpeg;base64,$(base64 DIP20-logo.png)\",
        \"DIP20 Token\",
        \"TKN\",
        8:nat8,
        $GENESIS_AMT:nat,
        principal \"$(dfx identity get-principal)\", 
        0, 
        principal \"$(dfx identity get-principal)\", 
        principal \"$CAP_ID\"
        )" \
    $MODE