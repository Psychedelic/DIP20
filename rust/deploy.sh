sudo dfx canister --no-wallet create --all
cargo run > token.did
ic-cdk-optimizer target/wasm32-unknown-unknown/release/token.wasm -o target/wasm32-unknown-unknown/release/opt.wasm
sudo dfx build token
OWNER="principal \"$( \
   dfx identity get-principal
)\""
CAP_ID="principal \"e22n6-waaaa-aaaah-qcd2q-cai\""

sudo dfx canister --no-wallet install token --argument "(
   \"test logo\", 
   \"test token\", 
   \"TT\", 
   8:nat8, 
   100000000:nat, 
   $OWNER, 
   0,
   $OWNER, 
   $CAP_ID, 
)" -m=reinstall