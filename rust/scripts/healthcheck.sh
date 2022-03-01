#/bin/bash
# --- HEALTHCHECK.SH --- 

set -e # exit when any command fails
NETWORK="local"

deploy() {
    ./scripts/deploy.sh $NETWORK
}

info() {
    printf "💎 DIP20 Canister Info:\n\n"
    printf "Name: "
    dfx canister --network $NETWORK --no-wallet call token name
    printf "Symbol: "
    dfx canister --network $NETWORK --no-wallet call token symbol
    printf "Total Supply: "
    dfx canister --network $NETWORK --no-wallet call token totalSupply
    printf "Decimals: "
    dfx canister --network $NETWORK --no-wallet call token decimals
    printf "History Size: "
    dfx canister --network $NETWORK --no-wallet call token historySize
}

balance() {
    printf "💎 Balances :\n\n"
    printf "Default Balance: "
    dfx canister --network $NETWORK --no-wallet call token balanceOf "(principal \"$DEFAULT_PRINCIPAL_ID\")"
    printf "Alice Balance: "
    dfx canister --network $NETWORK --no-wallet call token balanceOf "(principal \"$ALICE_PRINCIPAL_ID\")"
    printf "Bob Balance: "
    dfx canister --network $NETWORK --no-wallet call token balanceOf "(principal \"$BOB_PRINCIPAL_ID\")"
}

allowance() {
    printf "Allow Alice access 10000 token from the dfx principal id\n"
    dfx canister --network $NETWORK --no-wallet call token approve "(principal \"$ALICE_PRINCIPAL_ID\", 10000:nat)"
}

transferFrom() {
    printf "Transfer 1000 token from dfx principal id to Bob, as user Alice\n"
    HOME=$ALICE_HOME dfx canister --network $NETWORK --no-wallet call token transferFrom "(principal \"$DEFAULT_PRINCIPAL_ID\",principal \"$BOB_PRINCIPAL_ID\", 1000:nat)"
}

transfer() {
    printf "Transfer 500 token Bob -> Alice\n"
    HOME=$BOB_HOME dfx canister --network $NETWORK --no-wallet call token transfer "(principal \"$ALICE_PRINCIPAL_ID\", 500:nat)"
}

metadata() {
    printf "Metadata: %s\n\n" "$(dfx canister --network $NETWORK --no-wallet call token getMetadata)"
}


tests() {
    deploy
    info
    balance
    allowance
    transferFrom
    transfer
    balance
}

. ./scripts/identity.sh # setup temporary identities 
tests