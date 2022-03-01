#!/bin/bash

DFX_IDENTITY=$(dfx identity whoami)

# Bob and alice are newly created each time
ALICE_HOME=$(mktemp -d 2>/dev/null || mktemp -d -t alice-temp)
BOB_HOME=$(mktemp -d 2>/dev/null || mktemp -d -t bob-temp)

DEFAULT_HOME="$HOME"

ALICE_PRINCIPAL_ID=$(HOME=$ALICE_HOME dfx identity get-principal)
BOB_PRINCIPAL_ID=$(HOME=$BOB_HOME dfx identity get-principal)
DEFAULT_PRINCIPAL_ID=$(HOME=$HOME dfx identity get-principal)

ALICE_PEM="$ALICE_HOME/.config/dfx/identity/default/identity.pem"
BOB_PEM="$BOB_HOME/.config/dfx/identity/default/identity.pem"
DEFAULT_PEM="$HOME/.config/dfx/identity/$DFX_IDENTITY/identity.pem"

printf "🙋‍♀️ Identities ↴\n\n"

printf "👩🏽‍🦰 ALICE_PRINCIPAL_ID (%s)\n" "$ALICE_PRINCIPAL_ID"
printf "👩🏽‍🦰 ALICE_HOME (%s)\n" "$ALICE_HOME"

printf "👨🏽‍🦰 BOB_PRINCIPAL_ID (%s)\n" "$BOB_PRINCIPAL_ID"
printf "👨🏽‍🦰 BOB_HOME (%s)\n" "$BOB_HOME"

printf "👨🏾‍💻 DFX_IDENTITY (%s)\n" "$DFX_IDENTITY"
printf "👨🏾‍💻 PRINCIPAL_ID (%s)\n" "$DEFAULT_PRINCIPAL_ID"
printf "👨🏾‍💻 HOME (%s)\n" "$DEFAULT_HOME"

printf "\n\n"