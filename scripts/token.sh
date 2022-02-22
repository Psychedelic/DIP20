#!/bin/bash

printf "DIP20 Copyright (C) 2022 Fleek LLC
    This program comes with ABSOLUTELY NO WARRANTY
    This is free software, and you are welcome to redistribute it
    under certain conditions;\n\n"

dfxDir="/home/dan/.config/dfx"
candidDir="/home/dan/dev/psy/ic-token/rust"

tokenID=$(dfx canister id token)
AlicePem="${dfxDir}/identity/Alice/identity.pem"
BobPem="${dfxDir}/identity/Bob/identity.pem"
CharliePem="${dfxDir}/identity/Charlie/identity.pem"
tokenCandidFile="${candidDir}/token.did"
AlicePrincipalId=$(dfx identity use Alice 2>/dev/null;dfx identity get-principal)
BobPrincipalId=$(dfx identity use Bob 2>/dev/null;dfx identity get-principal)
CharliePrincipalId=$(dfx identity use Charlie 2>/dev/null;dfx identity get-principal)
tokenPrologueXtc="--candid=${tokenCandidFile}"

dfx identity use default 2>/dev/null

declare -A nameToPrincipal=( ["Alice"]="$AlicePrincipalId" ["Bob"]="$BobPrincipalId" ["Charlie"]="$CharliePrincipalId")
declare -A nameToPem=( ["Alice"]="$AlicePem" ["Bob"]="$BobPem" ["Charlie"]="$CharliePem")

allowance(){
    pem=$AlicePem
    from="${nameToPrincipal[$1]}"
    to="${nameToPrincipal[$2]}"
    icx --pem=$pem query $tokenID allowance "(principal \"$from\", principal \"$to\")" $tokenPrologueXtc
}

decimals(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID decimals "()" $tokenPrologueXtc
}

getMetadata(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID getMetadata "()" $tokenPrologueXtc
}

historySize(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID historySize "()" $tokenPrologueXtc
}

logo(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID logo "()" $tokenPrologueXtc
}

name(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID nameErc20 "()" $tokenPrologueXtc
}

symbol(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID symbol "()" $tokenPrologueXtc
}

totalSupply(){
    pem=$AlicePem
    icx --pem=$pem query $tokenID totalSupply "()" $tokenPrologueXtc
}

getTransaction(){
	txId=$1
	pem=$AlicePem
	icx --pem=$pem update $tokenID getTransaction "($txId)" $tokenPrologueXtc
}

getTransactions(){
	txId=$1
    limit=$2
	pem=$AlicePem
	icx --pem=$pem update $tokenID getTransactions "($txId, $limit)" $tokenPrologueXtc
}

approve(){
	pem="${nameToPem[$1]}"
	to="${nameToPrincipal[$2]}"
	amount=$3
	icx --pem=$pem update $tokenID approve "(principal \"$to\", $amount)" $tokenPrologueXtc
}

transfer(){
	fromPem="${nameToPem[$1]}"
	to="${nameToPrincipal[$2]}"
	amount=$3
	icx --pem=$fromPem update $tokenID transfer "(principal \"$to\", $amount)" $tokenPrologueXtc
}

transferFrom(){
	from="${nameToPrincipal[$1]}"
	to="${nameToPrincipal[$2]}"
	amount=$3
	callerPem="${nameToPem[$1]}"
	if [ "$#" -eq 4 ]; then
    	callerPem="${nameToPem[$4]}"
	fi
	icx --pem=$callerPem update $tokenID transferFrom "(principal \"$from\",principal \"$to\", $amount)" $tokenPrologueXtc
}

balanceOf(){
	pem=$AlicePem
	account="${nameToPrincipal[$1]}"
	icx --pem=$pem query $tokenID balanceOf "(principal \"$account\")" $tokenPrologueXtc
}

help()
{
    printf "\n\nPrincipal ids\n"
    printf "Alice: ${AlicePrincipalId}\n"
    printf "Bob: ${BobPrincipalId}\n"
    printf "Charlie: ${CharliePrincipalId}\n"

    printf "Principal ids: ${nameToPrincipal}\n\n\n"
}
