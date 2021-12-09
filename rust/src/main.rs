/**
* Module     : main.rs
* Copyright  : 2021 DFinance Team
* License    : Apache 2.0 with LLVM Exception
* Maintainer : DFinance Team <hello@dfinance.ai>
* Stability  : Experimental
*/
use candid::{candid_method, CandidType, Deserialize, Int, Nat};
use cap_sdk::{handshake, insert, Event, IndefiniteEvent, TypedEvent};
use cap_std::dip20::cap::DIP20Details;
use cap_std::dip20::{Operation, TransactionStatus, TxRecord};
use ic_cdk_macros::*;
use ic_kit::{ic, Principal};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert::Into;
use std::iter::FromIterator;
use std::string::String;

#[derive(CandidType, Default, Deserialize)]
pub struct TxLog {
    pub ie_records: VecDeque<IndefiniteEvent>,
}

pub fn tx_log<'a>() -> &'a mut TxLog {
    ic_kit::ic::get_mut::<TxLog>()
}

#[derive(Deserialize, CandidType, Clone, Debug)]
struct Metadata {
    logo: String,
    name: String,
    symbol: String,
    decimals: u8,
    total_supply: Nat,
    owner: Principal,
    fee: Nat,
    fee_to: Principal,
    history_size: usize,
    deploy_time: u64,
}

#[derive(Deserialize, CandidType, Clone, Debug)]
struct TokenInfo {
    metadata: Metadata,
    // status info
    holder_number: usize,
    cycles: u64,
}

impl Default for Metadata {
    fn default() -> Self {
        Metadata {
            logo: "".to_string(),
            name: "".to_string(),
            symbol: "".to_string(),
            decimals: 0u8,
            total_supply: Nat::from(0),
            owner: Principal::anonymous(),
            fee: Nat::from(0),
            fee_to: Principal::anonymous(),
            history_size: 0,
            deploy_time: 0,
        }
    }
}

type Balances = HashMap<Principal, Nat>;
type Allowances = HashMap<Principal, HashMap<Principal, Nat>>;

#[derive(CandidType, Debug, PartialEq)]
pub enum TxError {
    InsufficientBalance,
    InsufficientAllowance,
    Unauthorized,
    LedgerTrap,
    AmountTooSmall,
    BlockUsed,
    ErrorOperationStyle,
    ErrorTo,
    Other,
}
pub type TxReceipt = Result<Nat, TxError>;

#[init]
#[candid_method(init)]
fn init(
    logo: String,
    name: String,
    symbol: String,
    decimals: u8,
    total_supply: Nat,
    owner: Principal,
    fee: Nat,
    fee_to: Principal,
    cap: Principal,
) {
    let metadata = ic::get_mut::<Metadata>();
    metadata.logo = logo;
    metadata.name = name;
    metadata.symbol = symbol;
    metadata.decimals = decimals;
    metadata.total_supply = total_supply.clone();
    metadata.owner = owner;
    metadata.fee = fee;
    metadata.fee_to = fee_to;
    metadata.history_size = 1;
    metadata.deploy_time = ic::time();
    handshake(1_000_000, Some(cap));
    let balances = ic::get_mut::<Balances>();
    balances.insert(owner, total_supply.clone());
    let _ = add_record(
        owner,
        Operation::Mint,
        owner,
        owner,
        total_supply,
        Nat::from(0),
        ic::time(),
        TransactionStatus::Succeeded,
    );
}

fn _transfer(from: Principal, to: Principal, value: Nat) {
    let balances = ic::get_mut::<Balances>();
    let from_balance = balance_of(from);
    let from_balance_new = from_balance - value.clone();
    if from_balance_new != 0 {
        balances.insert(from, from_balance_new);
    } else {
        balances.remove(&from);
    }
    let to_balance = balance_of(to);
    let to_balance_new = to_balance + value;
    if to_balance_new != 0 {
        balances.insert(to, to_balance_new);
    }
}

fn _charge_fee(user: Principal, fee_to: Principal, fee: Nat) {
    let metadata = ic::get::<Metadata>();
    if metadata.fee > Nat::from(0) {
        _transfer(user, fee_to, fee);
    }
}

#[update(name = "transfer")]
#[candid_method(update)]
async fn transfer(to: Principal, value: Nat) -> TxReceipt {
    let from = ic::caller();
    let metadata = ic::get_mut::<Metadata>();
    if balance_of(from) < value.clone() + metadata.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from, metadata.fee_to, metadata.fee.clone());
    _transfer(from, to, value.clone());
    metadata.history_size += 1;

    add_record(
        from,
        Operation::Transfer,
        from,
        to,
        value,
        metadata.fee.clone(),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "transferFrom")]
#[candid_method(update, rename = "transferFrom")]
async fn transfer_from(from: Principal, to: Principal, value: Nat) -> TxReceipt {
    let owner = ic::caller();
    let from_allowance = allowance(from, owner);
    let metadata = ic::get_mut::<Metadata>();
    if from_allowance < value.clone() + metadata.fee.clone() {
        return Err(TxError::InsufficientAllowance);
    }
    let from_balance = balance_of(from);
    if from_balance < value.clone() + metadata.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from, metadata.fee_to, metadata.fee.clone());
    _transfer(from, to, value.clone());
    let allowances = ic::get_mut::<Allowances>();
    match allowances.get(&from) {
        Some(inner) => {
            let result = inner.get(&owner).unwrap().clone();
            let mut temp = inner.clone();
            if result.clone() - value.clone() - metadata.fee.clone() != 0 {
                temp.insert(owner, result.clone() - value.clone() - metadata.fee.clone());
                allowances.insert(from, temp);
            } else {
                temp.remove(&owner);
                if temp.len() == 0 {
                    allowances.remove(&from);
                } else {
                    allowances.insert(from, temp);
                }
            }
        }
        None => {
            assert!(false);
        }
    }
    metadata.history_size += 1;

    add_record(
        owner,
        Operation::TransferFrom,
        from,
        to,
        value,
        metadata.fee.clone(),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "approve")]
#[candid_method(update)]
async fn approve(spender: Principal, value: Nat) -> TxReceipt {
    let owner = ic::caller();
    let metadata = ic::get_mut::<Metadata>();
    if balance_of(owner) < metadata.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(owner, metadata.fee_to, metadata.fee.clone());
    let v = value.clone() + metadata.fee.clone();
    let allowances = ic::get_mut::<Allowances>();
    match allowances.get(&owner) {
        Some(inner) => {
            let mut temp = inner.clone();
            if v.clone() != 0 {
                temp.insert(spender, v.clone());
                allowances.insert(owner, temp);
            } else {
                temp.remove(&spender);
                if temp.len() == 0 {
                    allowances.remove(&owner);
                } else {
                    allowances.insert(owner, temp);
                }
            }
        }
        None => {
            if v.clone() != 0 {
                let mut inner = HashMap::new();
                inner.insert(spender, v.clone());
                let allowances = ic::get_mut::<Allowances>();
                allowances.insert(owner, inner);
            }
        }
    }
    metadata.history_size += 1;

    add_record(
        owner,
        Operation::Approve,
        owner,
        spender,
        v,
        metadata.fee.clone(),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "mint")]
#[candid_method(update, rename = "mint")]
async fn mint(to: Principal, amount: Nat) -> TxReceipt {
    let caller = ic::caller();
    let metadata = ic::get_mut::<Metadata>();
    if caller != metadata.owner {
        return Err(TxError::Unauthorized);
    }
    let to_balance = balance_of(to);
    let balances = ic::get_mut::<Balances>();
    balances.insert(to, to_balance + amount.clone());
    metadata.total_supply += amount.clone();
    metadata.history_size += 1;

    add_record(
        caller,
        Operation::Mint,
        caller,
        to,
        amount,
        Nat::from(0),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "burn")]
#[candid_method(update, rename = "burn")]
async fn burn(amount: Nat) -> TxReceipt {
    let caller = ic::caller();
    let metadata = ic::get_mut::<Metadata>();
    let caller_balance = balance_of(caller);
    if caller_balance.clone() < amount.clone() {
        return Err(TxError::InsufficientBalance);
    }
    let balances = ic::get_mut::<Balances>();
    balances.insert(caller, caller_balance - amount.clone());
    metadata.total_supply -= amount.clone();
    metadata.history_size += 1;

    add_record(
        caller,
        Operation::Burn,
        caller,
        caller,
        amount,
        Nat::from(0),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "setName")]
#[candid_method(update, rename = "setName")]
fn set_name(name: String) {
    let metadata = ic::get_mut::<Metadata>();
    assert_eq!(ic::caller(), metadata.owner);
    metadata.name = name;
}

#[update(name = "setLogo")]
#[candid_method(update, rename = "setLogo")]
fn set_logo(logo: String) {
    let metadata = ic::get_mut::<Metadata>();
    assert_eq!(ic::caller(), metadata.owner);
    metadata.logo = logo;
}

#[update(name = "setFee")]
#[candid_method(update, rename = "setFee")]
fn set_fee(fee: Nat) {
    let metadata = ic::get_mut::<Metadata>();
    assert_eq!(ic::caller(), metadata.owner);
    metadata.fee = fee;
}

#[update(name = "setFeeTo")]
#[candid_method(update, rename = "setFeeTo")]
fn set_fee_to(fee_to: Principal) {
    let metadata = ic::get_mut::<Metadata>();
    assert_eq!(ic::caller(), metadata.owner);
    metadata.fee_to = fee_to;
}

#[update(name = "setOwner")]
#[candid_method(update, rename = "setOwner")]
fn set_owner(owner: Principal) {
    let metadata = ic::get_mut::<Metadata>();
    assert_eq!(ic::caller(), metadata.owner);
    metadata.owner = owner;
}

#[query(name = "balanceOf")]
#[candid_method(query, rename = "balanceOf")]
fn balance_of(id: Principal) -> Nat {
    let balances = ic::get::<Balances>();
    match balances.get(&id) {
        Some(balance) => balance.clone(),
        None => Nat::from(0),
    }
}

#[query(name = "allowance")]
#[candid_method(query)]
fn allowance(owner: Principal, spender: Principal) -> Nat {
    let allowances = ic::get::<Allowances>();
    match allowances.get(&owner) {
        Some(inner) => match inner.get(&spender) {
            Some(value) => value.clone(),
            None => Nat::from(0),
        },
        None => Nat::from(0),
    }
}

#[query(name = "getLogo")]
#[candid_method(query, rename = "getLogo")]
fn get_logo() -> String {
    let metadata = ic::get::<Metadata>();
    metadata.logo.clone()
}

#[query(name = "name")]
#[candid_method(query)]
fn name() -> String {
    let metadata = ic::get::<Metadata>();
    metadata.name.clone()
}

#[query(name = "symbol")]
#[candid_method(query)]
fn symbol() -> String {
    let metadata = ic::get::<Metadata>();
    metadata.symbol.clone()
}

#[query(name = "decimals")]
#[candid_method(query)]
fn decimals() -> u8 {
    let metadata = ic::get::<Metadata>();
    metadata.decimals
}

#[query(name = "totalSupply")]
#[candid_method(query, rename = "totalSupply")]
fn total_supply() -> Nat {
    let metadata = ic::get::<Metadata>();
    metadata.total_supply.clone()
}

#[query(name = "owner")]
#[candid_method(query)]
fn owner() -> Principal {
    let metadata = ic::get::<Metadata>();
    metadata.owner
}

#[query(name = "getMetadta")]
#[candid_method(query, rename = "getMetadta")]
fn get_metadata() -> Metadata {
    ic::get::<Metadata>().clone()
}

#[query(name = "historySize")]
#[candid_method(query, rename = "historySize")]
fn history_size() -> usize {
    let metadata = ic::get::<Metadata>();
    metadata.history_size
}

#[query(name = "getTokenInfo")]
#[candid_method(query, rename = "getTokenInfo")]
fn get_token_info() -> TokenInfo {
    let metadata = ic::get::<Metadata>().clone();
    let balance = ic::get::<Balances>();

    return TokenInfo {
        metadata: metadata.clone(),
        holder_number: balance.len(),
        cycles: ic::balance(),
    };
}

#[query(name = "getHolders")]
#[candid_method(query, rename = "getHolders")]
fn get_holders(start: usize, limit: usize) -> Vec<(Principal, Nat)> {
    let mut balance = Vec::new();
    for (k, v) in ic::get::<Balances>().clone() {
        balance.push((k, v));
    }
    balance.sort_by(|a, b| b.1.cmp(&a.1));
    let limit: usize = if start + limit > balance.len() {
        balance.len() - start
    } else {
        limit
    };
    balance[start..start + limit].to_vec()
}

#[query(name = "getAllowanceSize")]
#[candid_method(query, rename = "getAllowanceSize")]
fn get_allowance_size() -> usize {
    let mut size = 0;
    let allowances = ic::get::<Allowances>();
    for (_, v) in allowances.iter() {
        size += v.len();
    }
    size
}

#[query(name = "getUserApprovals")]
#[candid_method(query, rename = "getUserApprovals")]
fn get_user_approvals(who: Principal) -> Vec<(Principal, Nat)> {
    let allowances = ic::get::<Allowances>();
    match allowances.get(&who) {
        Some(allow) => return Vec::from_iter(allow.clone().into_iter()),
        None => return Vec::new(),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn main() {}

#[cfg(not(any(target_arch = "wasm32", test)))]
fn main() {
    candid::export_service!();
    std::print!("{}", __export_service());
}

#[pre_upgrade]
fn pre_upgrade() {
    ic::stable_store((
        ic::get::<Metadata>().clone(),
        ic::get::<Balances>(),
        ic::get::<Allowances>(),
        tx_log(),
    ))
    .unwrap();
}

#[post_upgrade]
fn post_upgrade() {
    let (metadata_stored, balances_stored, allowances_stored, tx_log_stored): (
        Metadata,
        Balances,
        Allowances,
        TxLog,
    ) = ic::stable_restore().unwrap();
    let metadata = ic::get_mut::<Metadata>();
    *metadata = metadata_stored;

    let balances = ic::get_mut::<Balances>();
    *balances = balances_stored;

    let allowances = ic::get_mut::<Allowances>();
    *allowances = allowances_stored;

    let tx_log = tx_log();
    *tx_log = tx_log_stored;
}

async fn add_record(
    caller: Principal,
    op: Operation,
    from: Principal,
    to: Principal,
    amount: Nat,
    fee: Nat,
    timestamp: u64,
    status: TransactionStatus,
) -> TxReceipt {
    insert_into_cap(Into::<IndefiniteEvent>::into(Into::<Event>::into(Into::<
        TypedEvent<DIP20Details>,
    >::into(
        TxRecord {
            caller: Some(caller),
            index: Nat::from(0),
            from,
            to,
            amount: Nat::from(amount),
            fee: Nat::from(fee),
            timestamp: Int::from(timestamp),
            status,
            operation: op,
        },
    ))))
    .await
}

pub async fn insert_into_cap(ie: IndefiniteEvent) -> TxReceipt {
    let tx_log = tx_log();
    if let Some(failed_ie) = tx_log.ie_records.pop_front() {
        let _ = insert_into_cap_priv(failed_ie).await;
    }
    insert_into_cap_priv(ie).await
}

async fn insert_into_cap_priv(ie: IndefiniteEvent) -> TxReceipt {
    let insert_res = insert(ie.clone())
        .await
        .map(|tx_id| Nat::from(tx_id))
        .map_err(|_| TxError::Other);

    if insert_res.is_err() {
        tx_log().ie_records.push_back(ie.clone());
    }

    insert_res
}
