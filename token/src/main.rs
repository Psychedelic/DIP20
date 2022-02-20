/**
* Module     : main.rs
* Copyright  : 2021 DFinance Team
* License    : Apache 2.0 with LLVM Exception
* Maintainer : DFinance Team <hello@dfinance.ai>
* Stability  : Experimental
*/
use candid::{candid_method, CandidType, Deserialize, Int, Nat};
use cap_sdk::{handshake, insert, CapEnv, Event, IndefiniteEvent, TypedEvent};
use cap_std::dip20::cap::DIP20Details;
use cap_std::dip20::{Operation, TransactionStatus, TxRecord};
use dfn_core::api::call_with_cleanup;
use dfn_protobuf::protobuf;
use ic_cdk_macros::*;
use ic_kit::{ic, Principal};
use ic_types::{CanisterId, PrincipalId};
use ledger_canister::{
    account_identifier::{AccountIdentifier, Subaccount},
    tokens::Tokens,
    BlockHeight, BlockRes, Memo, Operation as Operate, SendArgs,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Into;
use std::iter::FromIterator;
use std::string::String;

#[derive(CandidType, Default, Deserialize, Clone)]
pub struct TxLog {
    pub ie_records: VecDeque<IndefiniteEvent>,
}

#[allow(non_snake_case)]
#[derive(Deserialize, CandidType, Clone, Debug)]
struct Metadata {
    logo: String,
    name: String,
    symbol: String,
    decimals: u8,
    totalSupply: Nat,
    owner: Principal,
    fee: Nat,
}

#[derive(Deserialize, CandidType, Clone, Debug)]
struct StatsData {
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

impl Default for StatsData {
    fn default() -> Self {
        StatsData {
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

#[allow(non_snake_case)]
#[derive(Deserialize, CandidType, Clone, Debug)]
struct TokenInfo {
    metadata: Metadata,
    feeTo: Principal,
    // status info
    historySize: usize,
    deployTime: u64,
    holderNumber: usize,
    cycles: u64,
}

struct Genesis {
    caller: Option<Principal>,
    op: Operation,
    from: Principal,
    to: Principal,
    amount: Nat,
    fee: Nat,
    timestamp: u64,
    status: TransactionStatus,
}

impl Default for Genesis {
    fn default() -> Self {
        Genesis {
            caller: None,
            op: Operation::Mint,
            from: Principal::anonymous(),
            to: Principal::anonymous(),
            amount: Nat::from(0),
            fee: Nat::from(0),
            timestamp: 0,
            status: TransactionStatus::Succeeded,
        }
    }
}

type Balances = HashMap<Principal, Nat>;
type Allowances = HashMap<Principal, HashMap<Principal, Nat>>;
type UsedBlocks = HashSet<BlockHeight>;

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

thread_local! {
    /*    stable    */
    static BALANCES: RefCell<HashMap<Principal, Nat>> = RefCell::new(HashMap::default());
    static ALLOWS: RefCell<HashMap<Principal, HashMap<Principal, Nat>>> = RefCell::new(HashMap::default());
    static STATS: RefCell<StatsData> = RefCell::new(StatsData::default());
    static TXLOG: RefCell<TxLog> = RefCell::new(TxLog::default());
    /*   flexible   */
}

const LEDGER_CANISTER_ID: CanisterId = CanisterId::from_u64(2);
const THRESHOLD: Tokens = Tokens::from_e8s(0); // 0;
const ICPFEE: Tokens = Tokens::from_e8s(10000);

#[init]
#[candid_method(init)]
fn init(
    logo: String,
    name: String,
    symbol: String,
    decimals: u8,
    initial_supply: Nat,
    owner: Principal,
    fee: Nat,
    fee_to: Principal,
    cap: Principal,
) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.logo = logo;
        stats.name = name;
        stats.symbol = symbol;
        stats.decimals = decimals;
        stats.total_supply = initial_supply.clone();
        stats.owner = owner;
        stats.fee = fee.clone();
        stats.fee_to = fee_to;
        stats.history_size = 1;
        stats.deploy_time = ic::time();
    });
    handshake(1_000_000_000_000, Some(cap));
    let balances = ic::get_mut::<Balances>();
    balances.insert(owner, initial_supply.clone());
    let genesis = ic::get_mut::<Genesis>();
    genesis.caller = Some(owner);
    genesis.op = Operation::Mint;
    genesis.from = Principal::from_text("aaaaa-aa").unwrap();
    genesis.to = owner;
    genesis.amount = initial_supply;
    genesis.fee = fee;
    genesis.timestamp = ic::time();
    genesis.status = TransactionStatus::Succeeded;
}

#[update(name = "transfer")]
#[candid_method(update)]
async fn transfer(to: Principal, value: Nat) -> TxReceipt {
    let from = ic::caller();
    let stats = ic::get_mut::<StatsData>();
    if balance_of(from) < value.clone() + stats.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from);
    _transfer(from, to, value.clone());
    _history_inc();

    add_record(
        Some(from),
        Operation::Transfer,
        from,
        to,
        value,
        stats.fee.clone(),
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
    let stats = ic::get_mut::<StatsData>();
    if from_allowance < value.clone() + stats.fee.clone() {
        return Err(TxError::InsufficientAllowance);
    }
    let from_balance = balance_of(from);
    if from_balance < value.clone() + stats.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from);
    _transfer(from, to, value.clone());
    let allowances = ic::get_mut::<Allowances>();
    match allowances.get(&from) {
        Some(inner) => {
            let result = inner.get(&owner).unwrap().clone();
            let mut temp = inner.clone();
            if result.clone() - value.clone() - stats.fee.clone() != 0 {
                temp.insert(owner, result - value.clone() - stats.fee.clone());
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
    _history_inc();
    add_record(
        Some(owner),
        Operation::TransferFrom,
        from,
        to,
        value,
        stats.fee.clone(),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "approve")]
#[candid_method(update)]
async fn approve(spender: Principal, value: Nat) -> TxReceipt {
    let owner = ic::caller();
    let stats = ic::get_mut::<StatsData>();
    if balance_of(owner) < stats.fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(owner);
    let v = value.clone() + stats.fee.clone();
    let allowances = ic::get_mut::<Allowances>();
    match allowances.get(&owner) {
        Some(inner) => {
            let mut temp = inner.clone();
            if v != 0 {
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
            if v != 0 {
                let mut inner = HashMap::new();
                inner.insert(spender, v.clone());
                let allowances = ic::get_mut::<Allowances>();
                allowances.insert(owner, inner);
            }
        }
    }
    _history_inc();
    add_record(
        Some(owner),
        Operation::Approve,
        owner,
        spender,
        v,
        stats.fee.clone(),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "mint")]
#[candid_method(update, rename = "mint")]
async fn mint(sub_account: Option<Subaccount>, block_height: BlockHeight) -> TxReceipt {
    let caller = ic::caller();

    let response: Result<BlockRes, (Option<i32>, String)> =
        call_with_cleanup(LEDGER_CANISTER_ID, "block_pb", protobuf, block_height).await;
    let encode_block = match response {
        Ok(BlockRes(res)) => match res {
            Some(result_encode_block) => match result_encode_block {
                Ok(encode_block) => encode_block,
                Err(e) => {
                    let storage = match Principal::from_text(e.to_string()) {
                        Ok(p) => p,
                        Err(_) => return Err(TxError::Other),
                    };
                    let storage_canister = match CanisterId::new(PrincipalId::from(storage)) {
                        Ok(c) => c,
                        Err(_) => return Err(TxError::Other),
                    };
                    let response: Result<BlockRes, (Option<i32>, String)> =
                        call_with_cleanup(storage_canister, "get_block_pb", protobuf, block_height)
                            .await;
                    match response {
                        Ok(BlockRes(res)) => match res {
                            Some(result_encode_block) => match result_encode_block {
                                Ok(encode_block) => encode_block,
                                Err(_) => return Err(TxError::Other),
                            },
                            None => return Err(TxError::Other),
                        },
                        Err(_) => return Err(TxError::Other),
                    }
                }
            },
            None => return Err(TxError::Other),
        },
        Err(_) => return Err(TxError::Other),
    };

    let block = match encode_block.decode() {
        Ok(block) => block,
        Err(_) => return Err(TxError::Other),
    };

    let (from, to, amount) = match block.transaction.operation {
        Operate::Transfer {
            from,
            to,
            amount,
            fee: _,
        } => (from, to, amount),
        _ => {
            return Err(TxError::ErrorOperationStyle);
        }
    };

    let blocks = ic::get_mut::<UsedBlocks>();
    assert_eq!(blocks.insert(block_height), true);

    let caller_pid = PrincipalId::from(caller);
    let caller_account = AccountIdentifier::new(caller_pid, sub_account);

    if caller_account != from {
        blocks.remove(&block_height);
        return Err(TxError::Unauthorized);
    }

    if AccountIdentifier::new(PrincipalId::from(ic::id()), None) != to {
        blocks.remove(&block_height);
        return Err(TxError::ErrorTo);
    }

    if amount < THRESHOLD {
        blocks.remove(&block_height);
        return Err(TxError::AmountTooSmall);
    }

    let value = Nat::from(Tokens::get_e8s(amount));

    let user_balance = balance_of(caller);
    let balances = ic::get_mut::<Balances>();
    balances.insert(caller, user_balance + value.clone());
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.total_supply += value.clone();
        stats.history_size += 1;
    });

    add_record(
        Some(caller),
        Operation::Mint,
        caller,
        caller,
        value,
        Nat::from(0),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "mintFor")]
#[candid_method(update, rename = "mintFor")]
async fn mint_for(
    sub_account: Option<Subaccount>,
    block_height: BlockHeight,
    to_p: Principal,
) -> TxReceipt {
    let caller = ic::caller();

    let response: Result<BlockRes, (Option<i32>, String)> =
        call_with_cleanup(LEDGER_CANISTER_ID, "block_pb", protobuf, block_height).await;
    let encode_block = match response {
        Ok(BlockRes(res)) => match res {
            Some(result_encode_block) => match result_encode_block {
                Ok(encode_block) => encode_block,
                Err(e) => {
                    let storage = match Principal::from_text(e.to_string()) {
                        Ok(p) => p,
                        Err(_) => return Err(TxError::Other),
                    };
                    let storage_canister = match CanisterId::new(PrincipalId::from(storage)) {
                        Ok(c) => c,
                        Err(_) => return Err(TxError::Other),
                    };
                    let response: Result<BlockRes, (Option<i32>, String)> =
                        call_with_cleanup(storage_canister, "get_block_pb", protobuf, block_height)
                            .await;
                    match response {
                        Ok(BlockRes(res)) => match res {
                            Some(result_encode_block) => match result_encode_block {
                                Ok(encode_block) => encode_block,
                                Err(_) => return Err(TxError::Other),
                            },
                            None => return Err(TxError::Other),
                        },
                        Err(_) => return Err(TxError::Other),
                    }
                }
            },
            None => return Err(TxError::Other),
        },
        Err(_) => return Err(TxError::Other),
    };

    let block = match encode_block.decode() {
        Ok(block) => block,
        Err(_) => return Err(TxError::Other),
    };

    let (from, to, amount) = match block.transaction.operation {
        Operate::Transfer {
            from,
            to,
            amount,
            fee: _,
        } => (from, to, amount),
        _ => {
            return Err(TxError::ErrorOperationStyle);
        }
    };

    let blocks = ic::get_mut::<UsedBlocks>();
    assert_eq!(blocks.insert(block_height), true);

    let to_pid = PrincipalId::from(to_p);
    let to_account = AccountIdentifier::new(to_pid, sub_account);

    if to_account != from {
        blocks.remove(&block_height);
        return Err(TxError::Unauthorized);
    }

    if AccountIdentifier::new(PrincipalId::from(ic::id()), None) != to {
        blocks.remove(&block_height);
        return Err(TxError::ErrorTo);
    }

    if amount < THRESHOLD {
        blocks.remove(&block_height);
        return Err(TxError::AmountTooSmall);
    }

    let value = Nat::from(Tokens::get_e8s(amount));

    let user_balance = balance_of(to_p);
    let balances = ic::get_mut::<Balances>();
    balances.insert(to_p, user_balance + value.clone());
    let stats = ic::get_mut::<StatsData>();
    stats.total_supply += value.clone();
    stats.history_size += 1;

    add_record(
        Some(caller),
        Operation::Mint,
        to_p,
        to_p,
        value,
        Nat::from(0),
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "withdraw")]
#[candid_method(update, rename = "withdraw")]
async fn withdraw(value: u64, to: String) -> TxReceipt {
    if Tokens::from_e8s(value) < THRESHOLD {
        return Err(TxError::AmountTooSmall);
    }
    let caller = ic::caller();
    let caller_balance = balance_of(caller);
    let value_nat = Nat::from(value);
    let stats = ic::get_mut::<StatsData>();
    if caller_balance.clone() < value_nat.clone() || stats.total_supply < value_nat.clone() {
        return Err(TxError::InsufficientBalance);
    }
    let args = SendArgs {
        memo: Memo(0x57444857),
        amount: (Tokens::from_e8s(value) - ICPFEE).unwrap(),
        fee: ICPFEE,
        from_subaccount: None,
        to: AccountIdentifier::from_hex(&to).unwrap(),
        created_at_time: None,
    };
    let balances = ic::get_mut::<Balances>();
    balances.insert(caller, caller_balance.clone() - value_nat.clone());
    stats.total_supply -= value_nat.clone();
    let result: Result<(u64,), _> = ic::call(
        Principal::from(CanisterId::get(LEDGER_CANISTER_ID)),
        "send_dfx",
        (args,),
    )
    .await;
    match result {
        Ok(_) => {
            _history_inc();
            add_record(
                Some(caller),
                Operation::Burn,
                caller,
                caller,
                value_nat,
                Nat::from(0),
                ic::time(),
                TransactionStatus::Succeeded,
            )
            .await
        }
        Err(_) => {
            balances.insert(caller, balance_of(caller) + value_nat.clone());
            stats.total_supply += value_nat;
            return Err(TxError::LedgerTrap);
        }
    }
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
    allowances
        .get(&owner)
        .unwrap_or(&HashMap::new())
        .get(&spender)
        .unwrap_or(&Nat::from(0))
        .clone()
}

#[query]
#[candid_method(query)]
fn logo() -> String {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.logo.clone()
    })
}

#[query(name = "name")]
#[candid_method(query)]
fn name() -> String {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.name.clone()
    })
}

#[query(name = "symbol")]
#[candid_method(query)]
fn symbol() -> String {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.symbol.clone()
    })
}

#[query(name = "decimals")]
#[candid_method(query)]
fn decimals() -> u8 {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.decimals
    })
}

#[query(name = "totalSupply")]
#[candid_method(query, rename = "totalSupply")]
fn total_supply() -> Nat {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.total_supply.clone()
    })
}

#[query(name = "owner")]
#[candid_method(query)]
fn owner() -> Principal {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.owner
    })
}

#[query(name = "getMetadata")]
#[candid_method(query, rename = "getMetadata")]
fn get_metadata() -> Metadata {
    STATS.with(|s| {
        let stats = s.borrow().clone();
        Metadata {
            logo: stats.logo,
            name: stats.name,
            symbol: stats.symbol,
            decimals: stats.decimals,
            totalSupply: stats.total_supply,
            owner: stats.owner,
            fee: stats.fee,
        }
    })
}

#[query(name = "historySize")]
#[candid_method(query, rename = "historySize")]
fn history_size() -> usize {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.history_size
    })
}

#[query(name = "getTokenInfo")]
#[candid_method(query, rename = "getTokenInfo")]
fn get_token_info() -> TokenInfo {
    let stats = ic::get::<StatsData>().clone();
    let balance = ic::get::<Balances>();

    return TokenInfo {
        metadata: get_metadata(),
        feeTo: stats.fee_to,
        historySize: stats.history_size,
        deployTime: stats.deploy_time,
        holderNumber: balance.len(),
        cycles: ic::balance(),
    };
}

#[query(name = "getHolders")]
#[candid_method(query, rename = "getHolders")]
fn get_holders(start: usize, limit: usize) -> Vec<(Principal, Nat)> {
    let mut balance = Vec::new();
    for (k, v) in ic::get::<Balances>().clone() {
        balance.push((k, v.clone()));
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

#[query(name = "getBlockUsed")]
#[candid_method(query, rename = "getBlockUsed")]
fn get_block_used() -> HashSet<u64> {
    ic::get::<UsedBlocks>().clone()
}

#[query(name = "isBlockUsed")]
#[candid_method(query, rename = "isBlockUsed")]
fn is_block_used(block_number: BlockHeight) -> bool {
    ic::get::<UsedBlocks>().contains(&block_number)
}

/* PERMISSIONED FNS */

#[update(name = "setName", guard = _is_auth)]
#[candid_method(update, rename = "setName")]
fn set_name(name: String) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.name = name;
    });
}

#[update(name = "setLogo", guard = _is_auth)]
#[candid_method(update, rename = "setLogo")]
fn set_logo(logo: String) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.logo = logo;
    });
}

#[update(name = "setFee", guard = _is_auth)]
#[candid_method(update, rename = "setFee")]
fn set_fee(fee: Nat) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.fee = fee;
    });
}

#[update(name = "setFeeTo", guard = _is_auth)]
#[candid_method(update, rename = "setFeeTo")]
fn set_fee_to(fee_to: Principal) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.fee_to = fee_to;
    });
}

#[update(name = "setOwner", guard = _is_auth)]
#[candid_method(update, rename = "setOwner")]
fn set_owner(owner: Principal) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.owner = owner;
    });
}

#[update(name = "setGenesis", guard = _is_auth)]
#[candid_method(update, rename = "setGenesis")]
async fn set_genesis() -> TxReceipt {
    let genesis = ic::get::<Genesis>();
    add_record(
        genesis.caller,
        genesis.op,
        genesis.from,
        genesis.to,
        genesis.amount.clone(),
        genesis.fee.clone(),
        genesis.timestamp,
        genesis.status,
    )
    .await
}

/* INTERNAL FNS */

// TODO: use controllers for ownership
// this will require the canister to be a controller of itself (like dip721)
fn _is_auth() -> Result<(), String> {
    STATS.with(|s| {
        let stats = s.borrow();
        if ic_cdk::api::caller() == stats.owner {
            Ok(())
        } else {
            Err("Error: Unauthorized principal ID".to_string())
        }
    })
}

pub fn tx_log<'a>() -> &'a mut TxLog {
    ic_kit::ic::get_mut::<TxLog>()
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

fn _charge_fee(user: Principal) {
    STATS.with(|s| {
        let stats = s.borrow();
        if stats.fee > Nat::from(0) {
            _transfer(user, stats.fee_to, stats.fee.clone());
        }
    });
}

fn _history_inc() {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.history_size += 1;
    })
}

fn _get_fee() -> Nat {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.fee.clone()
    })
}

fn _get_owner() -> Principal {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.owner
    })
}

async fn add_record(
    caller: Option<Principal>,
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
            caller,
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

/* MISC FNS */

#[cfg(any(target_arch = "wasm32", test))]
fn main() {}

#[cfg(not(any(target_arch = "wasm32", test)))]
fn main() {
    candid::export_service!();
    std::print!("{}", __export_service());
}

// TODO: fix upgrade functions
#[pre_upgrade]
fn pre_upgrade() {
    ic::stable_store((
        ic::get::<StatsData>().clone(),
        ic::get::<Balances>(),
        ic::get::<Allowances>(),
        ic::get::<UsedBlocks>(),
        tx_log(),
        CapEnv::to_archive(),
    ))
    .unwrap();
}

#[post_upgrade]
fn post_upgrade() {
    let (
        metadata_stored,
        balances_stored,
        allowances_stored,
        blocks_stored,
        tx_log_stored,
        cap_env,
    ): (StatsData, Balances, Allowances, UsedBlocks, TxLog, CapEnv) = ic::stable_restore().unwrap();
    let stats = ic::get_mut::<StatsData>();
    *stats = metadata_stored;

    let balances = ic::get_mut::<Balances>();
    *balances = balances_stored;

    let allowances = ic::get_mut::<Allowances>();
    *allowances = allowances_stored;

    let blocks = ic::get_mut::<UsedBlocks>();
    *blocks = blocks_stored;

    let tx_log = tx_log();
    *tx_log = tx_log_stored;

    CapEnv::load_from_archive(cap_env);
}

/* TESTS */

#[cfg(test)]
mod tests {
    use super::*;
    use assert_panic::assert_panic;
    use ic_kit::{
        mock_principals::{alice, bob, john},
        MockContext,
    };

    fn initialize_tests() {
        init(
            String::from("logo"),
            String::from("token"),
            String::from("TOKEN"),
            2,
            1_000,
            alice(),
            1,
        );
    }

    #[test]
    fn functionality_test() {
        MockContext::new()
            .with_balance(100_000)
            .with_caller(alice())
            .inject();

        initialize_tests();

        // initialization tests
        assert_eq!(
            balance_of(alice()),
            1_000,
            "balanceOf did not return the correct value"
        );
        assert_eq!(
            total_supply(),
            1_000,
            "totalSupply did not return the correct value"
        );
        assert_eq!(
            symbol(),
            String::from("TOKEN"),
            "symbol did not return the correct value"
        );
        assert_eq!(owner(), alice(), "owner did not return the correct value");
        assert_eq!(
            name(),
            String::from("token"),
            "name did not return the correct value"
        );
        assert_eq!(
            get_logo(),
            String::from("logo"),
            "getLogo did not return the correct value"
        );
        assert_eq!(decimals(), 2, "decimals did not return the correct value");
        assert_eq!(
            get_holders(0, 10).len(),
            1,
            "get_holders returned the correct amount of holders after initialization"
        );
        assert_eq!(
            get_transaction(0).op,
            Operation::Mint,
            "get_transaction returnded a Mint operation"
        );

        let token_info = get_token_info();
        assert_eq!(
            token_info.fee_to,
            Principal::anonymous(),
            "tokenInfo.fee_to did not return the correct value"
        );
        assert_eq!(
            token_info.history_size, 1,
            "tokenInfo.history_size did not return the correct value"
        );
        assert!(
            token_info.deploy_time > 0,
            "tokenInfo.deploy_time did not return the correct value"
        );
        assert_eq!(
            token_info.holder_number, 1,
            "tokenInfo.holder_number did not return the correct value"
        );
        assert_eq!(
            token_info.cycles, 100_000,
            "tokenInfo.cycles did not return the correct value"
        );

        let stats = get_metadata();
        assert_eq!(
            stats.total_supply, 1_000,
            "stats.total_supply did not return the correct value"
        );
        assert_eq!(
            stats.symbol,
            String::from("TOKEN"),
            "stats.symbol did not return the correct value"
        );
        // assert_eq!(stats.owner, alice(), "stats.owner did not return the correct value");
        assert_eq!(
            stats.name,
            String::from("token"),
            "stats.name did not return the correct value"
        );
        assert_eq!(
            stats.logo,
            String::from("logo"),
            "stats.logo did not return the correct value"
        );
        assert_eq!(
            stats.decimals, 2,
            "stats.decimals did not return the correct value"
        );
        assert_eq!(stats.fee, 1, "stats.fee did not return the correct value");
        assert_eq!(
            stats.fee_to,
            Principal::anonymous(),
            "stats.fee_to did not return the correct value"
        );

        // set fee test
        set_fee(2);
        assert_eq!(2, get_metadata().fee, "Failed to update the fee_to");

        // set fee_to test
        set_fee_to(john());
        assert_eq!(john(), get_metadata().fee_to, "Failed to set fee");
        set_fee_to(Principal::anonymous());

        // set logo
        set_logo(String::from("new_logo"));
        assert_eq!("new_logo", get_logo());

        // test transfers
        let transfer_alice_balance_expected = balance_of(alice()) - 10 - get_metadata().fee;
        let transfer_bob_balance_expected = balance_of(bob()) + 10;
        let transfer_john_balance_expected = balance_of(john());
        let transfer_transaction_amount_expected = get_transactions(0, 10).len() + 1;
        let transfer_user_transaction_amount_expected = get_user_transaction_amount(alice()) + 1;
        transfer(bob(), 10)
            .map_err(|err| println!("{:?}", err))
            .ok();

        assert_eq!(
            balance_of(alice()),
            transfer_alice_balance_expected,
            "Transfer did not transfer the expected amount to Alice"
        );
        assert_eq!(
            balance_of(bob()),
            transfer_bob_balance_expected,
            "Transfer did not transfer the expected amount to Bob"
        );
        assert_eq!(
            balance_of(john()),
            transfer_john_balance_expected,
            "Transfer did not transfer the expected amount to John"
        );
        assert_eq!(
            get_transactions(0, 10).len(),
            transfer_transaction_amount_expected,
            "transfer operation did not produce a transaction"
        );
        assert_eq!(
            get_user_transaction_amount(alice()),
            transfer_user_transaction_amount_expected,
            "get_user_transaction_amount returned the wrong value after a transfer"
        );
        assert_eq!(
            get_user_transactions(alice(), 0, 10).len(),
            transfer_user_transaction_amount_expected,
            "get_user_transactions returned the wrong value after a transfer"
        );
        assert_eq!(
            get_holders(0, 10).len(),
            3,
            "get_holders returned the correct amount of holders after transfer"
        );
        assert_eq!(
            get_transaction(1).op,
            Operation::Transfer,
            "get_transaction returnded a Transfer operation"
        );

        // test allowances
        approve(bob(), 100)
            .map_err(|err| println!("{:?}", err))
            .ok();
        assert_eq!(
            allowance(alice(), bob()),
            100 + get_metadata().fee,
            "Approve did not give the correct allowance"
        );
        assert_eq!(
            get_allowance_size(),
            1,
            "getAllowanceSize returns the correct value"
        );
        assert_eq!(
            get_user_approvals(alice()).len(),
            1,
            "getUserApprovals not returning the correct value"
        );

        // test transfer_from
        // inserting an allowance of Alice for Bob's balance to test transfer_from
        let allowances = ic::get_mut::<Allowances>();
        let mut inner = HashMap::new();
        inner.insert(alice(), 5 + get_metadata().fee);
        allowances.insert(bob(), inner);

        let transfer_from_alice_balance_expected = balance_of(alice());
        let transfer_from_bob_balance_expected = balance_of(bob()) - 5 - get_metadata().fee;
        let transfer_from_john_balance_expected = balance_of(john()) + 5;
        let transfer_from_transaction_amount_expected = get_transactions(0, 10).len() + 1;

        transfer_from(bob(), john(), 5)
            .map_err(|err| println!("{:?}", err))
            .ok();

        assert_eq!(
            balance_of(alice()),
            transfer_from_alice_balance_expected,
            "transfer_from transferred the correct value for alice"
        );
        assert_eq!(
            balance_of(bob()),
            transfer_from_bob_balance_expected,
            "transfer_from transferred the correct value for bob"
        );
        assert_eq!(
            balance_of(john()),
            transfer_from_john_balance_expected,
            "transfer_from transferred the correct value for john"
        );
        assert_eq!(allowance(bob(), alice()), 0, "allowance has not been spent");
        assert_eq!(
            get_transactions(0, 10).len(),
            transfer_from_transaction_amount_expected,
            "transfer_from operation did not produce a transaction"
        );

        // Transferring more than the balance
        assert_eq!(
            transfer(alice(), 1_000_000),
            Err(TxError::InsufficientBalance),
            "alice was able to transfer more than is allowed"
        );
        // Transferring more than the balance
        assert_eq!(
            transfer_from(bob(), john(), 1_000_000),
            Err(TxError::InsufficientAllowance),
            "alice was able to transfer more than is allowed"
        );

        //set owner test
        set_owner(bob());
        assert_eq!(bob(), owner(), "Failed to set new owner");
    }

    #[test]
    fn permission_tests() {
        MockContext::new()
            .with_balance(100_000)
            .with_caller(bob())
            .inject();

        initialize_tests();

        assert_panic!(set_logo(String::from("forbidden")));
        assert_panic!(set_fee(123));
        assert_panic!(set_fee_to(john()));
        assert_panic!(set_owner(bob()));
    }
}
