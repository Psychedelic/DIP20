/**
* Module     : main.rs
* Copyright  : 2022 Psychedelic
* License    : Apache 2.0 with LLVM Exception
* Maintainer : Ossian Mapes <oz@fleek.co>
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

#[derive(Deserialize, CandidType, Clone, Debug)]
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
    static BLOCKS: RefCell<HashSet<BlockHeight>> = RefCell::new(HashSet::default());
    static STATS: RefCell<StatsData> = RefCell::new(StatsData::default());
    static TXLOG: RefCell<TxLog> = RefCell::new(TxLog::default());
    static GENESIS: RefCell<Genesis> = RefCell::new(Genesis::default());
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
    _balance_ins(owner, initial_supply.clone());

    GENESIS.with(|g| {
        let mut genesis = g.borrow_mut();
        genesis.caller = Some(owner);
        genesis.op = Operation::Mint;
        genesis.from = Principal::from_text("aaaaa-aa").unwrap();
        genesis.to = owner;
        genesis.amount = initial_supply;
        genesis.fee = fee;
        genesis.timestamp = ic::time();
        genesis.status = TransactionStatus::Succeeded;
    });
}

#[update(name = "transfer")]
#[candid_method(update)]
async fn transfer(to: Principal, value: Nat) -> TxReceipt {
    let from = ic::caller();
    let fee = _get_fee();
    if balance_of(from) < value.clone() + fee.clone() {
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
        fee.clone(),
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
    let fee = _get_fee();
    if from_allowance < value.clone() + fee.clone() {
        return Err(TxError::InsufficientAllowance);
    }
    let from_balance = balance_of(from);
    if from_balance < value.clone() + fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(from);
    _transfer(from, to, value.clone());
    ALLOWS.with(|a| {
        let mut allowances = a.borrow_mut();
        match allowances.get(&from) {
            Some(inner) => {
                let result = inner.get(&owner).unwrap().clone();
                let mut temp = inner.clone();
                if result.clone() - value.clone() - fee.clone() != 0 {
                    temp.insert(owner, result - value.clone() - fee.clone());
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
    });
    _history_inc();
    add_record(
        Some(owner),
        Operation::TransferFrom,
        from,
        to,
        value,
        fee,
        ic::time(),
        TransactionStatus::Succeeded,
    )
    .await
}

#[update(name = "approve")]
#[candid_method(update)]
async fn approve(spender: Principal, value: Nat) -> TxReceipt {
    let owner = ic::caller();
    let fee = _get_fee();
    if balance_of(owner) < fee.clone() {
        return Err(TxError::InsufficientBalance);
    }
    _charge_fee(owner);
    let v = value.clone() + fee.clone();
    ALLOWS.with(|a| {
        let mut allowances = a.borrow_mut();
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
                    allowances.insert(owner, inner);
                }
            }
        }
    });
    _history_inc();
    add_record(
        Some(owner),
        Operation::Approve,
        owner,
        spender,
        v,
        fee,
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
    match BLOCKS.with(|b| {
        let mut blocks = b.borrow_mut();
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
        return Ok(());
    }) {
        Err(err) => return Err(err),
        _ => {
            let value = Nat::from(Tokens::get_e8s(amount));

            let user_balance = balance_of(caller);
            _balance_ins(caller, user_balance + value.clone());
            _supply_inc(value.clone());
            _history_inc();
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
    }
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
    match BLOCKS.with(|b| {
        let mut blocks = b.borrow_mut();
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
        return Ok(());
    }) {
        Err(err) => return Err(err),
        _ => {
            let value = Nat::from(Tokens::get_e8s(amount));

            let user_balance = balance_of(to_p);
            _balance_ins(to_p, user_balance + value.clone());
            _supply_inc(value.clone());
            _history_inc();
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
    }
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
    let total_supply = _supply_get();
    if caller_balance.clone() < value_nat.clone() || total_supply < value_nat.clone() {
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
    _balance_ins(caller, caller_balance.clone() - value_nat.clone());
    _supply_dec(value_nat.clone());
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
            _balance_ins(caller, balance_of(caller) + value_nat.clone());
            _supply_inc(value_nat);
            return Err(TxError::LedgerTrap);
        }
    }
}

#[query(name = "balanceOf")]
#[candid_method(query, rename = "balanceOf")]
fn balance_of(id: Principal) -> Nat {
    BALANCES.with(|b| {
        let balances = b.borrow();
        match balances.get(&id) {
            Some(balance) => balance.clone(),
            None => Nat::from(0),
        }
    })
}

#[query(name = "allowance")]
#[candid_method(query)]
fn allowance(owner: Principal, spender: Principal) -> Nat {
    ALLOWS.with(|a| {
        let allowances = a.borrow();
        allowances
            .get(&owner)
            .unwrap_or(&HashMap::new())
            .get(&spender)
            .unwrap_or(&Nat::from(0))
            .clone()
    })
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
    STATS.with(|s| {
        let stats = s.borrow();
        BALANCES.with(|b| {
            let balance = b.borrow();
            TokenInfo {
                metadata: get_metadata(),
                feeTo: stats.fee_to,
                historySize: stats.history_size,
                deployTime: stats.deploy_time,
                holderNumber: balance.len(),
                cycles: ic::balance(),
            }
        })
    })
}

#[query(name = "getHolders")]
#[candid_method(query, rename = "getHolders")]
fn get_holders(start: usize, limit: usize) -> Vec<(Principal, Nat)> {
    BALANCES.with(|b| {
        let balances = b.borrow();
        let mut bal = Vec::new();
        for (k, v) in balances.clone() {
            bal.push((k, v.clone()));
        }
        bal.sort_by(|a, b| b.1.cmp(&a.1));
        let limit: usize = if start + limit > bal.len() {
            bal.len() - start
        } else {
            limit
        };
        bal[start..start + limit].to_vec()
    })
}

#[query(name = "getAllowanceSize")]
#[candid_method(query, rename = "getAllowanceSize")]
fn get_allowance_size() -> usize {
    ALLOWS.with(|a| {
        let allowances = a.borrow();
        let mut size = 0;
        for (_, v) in allowances.iter() {
            size += v.len();
        }
        size
    })
}

#[query(name = "getUserApprovals")]
#[candid_method(query, rename = "getUserApprovals")]
fn get_user_approvals(who: Principal) -> Vec<(Principal, Nat)> {
    ALLOWS.with(|a| {
        let allowances = a.borrow();
        match allowances.get(&who) {
            Some(allow) => return Vec::from_iter(allow.clone().into_iter()),
            None => return Vec::new(),
        }
    })
}

#[query(name = "getBlockUsed")]
#[candid_method(query, rename = "getBlockUsed")]
fn get_block_used() -> HashSet<u64> {
    BLOCKS.with(|b| b.borrow().clone())
}

#[query(name = "isBlockUsed")]
#[candid_method(query, rename = "isBlockUsed")]
fn is_block_used(block_number: BlockHeight) -> bool {
    BLOCKS.with(|b| b.borrow().clone().contains(&block_number))
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
    let mut genesis = Genesis::default();
    GENESIS.with(|g| {
        genesis = g.borrow().clone();
    });
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

fn _balance_ins(from: Principal, value: Nat) {
    BALANCES.with(|b| {
        let mut balances = b.borrow_mut();
        balances.insert(from, value);
    });
}

fn _balance_rem(from: Principal) {
    BALANCES.with(|b| {
        let mut balances = b.borrow_mut();
        balances.remove(&from);
    });
}

fn _transfer(from: Principal, to: Principal, value: Nat) {
    let from_balance = balance_of(from);
    let from_balance_new = from_balance - value.clone();

    // TODO: check this logic ↴
    if from_balance_new != 0 {
        _balance_ins(from, from_balance_new);
    } else {
        _balance_rem(from)
    }
    let to_balance = balance_of(to);
    let to_balance_new = to_balance + value;
    if to_balance_new != 0 {
        _balance_ins(to, to_balance_new);
    }
}

fn _supply_inc(value: Nat) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.total_supply += value;
    })
}

fn _supply_dec(value: Nat) {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.total_supply -= value;
    })
}

fn _supply_get() -> Nat {
    STATS.with(|s| {
        let stats = s.borrow();
        stats.total_supply.clone()
    })
}

fn _history_inc() {
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        stats.history_size += 1;
    })
}

fn _charge_fee(user: Principal) {
    STATS.with(|s| {
        let stats = s.borrow();
        if stats.fee > Nat::from(0) {
            _transfer(user, stats.fee_to, stats.fee.clone());
        }
    });
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
    let mut event = ie;
    TXLOG.with(|t| {
        let mut tx_log = t.borrow_mut();
        if let Some(failed_ie) = tx_log.ie_records.pop_front() {
            event = failed_ie;
        }
    });
    insert_into_cap_priv(event).await
}

async fn insert_into_cap_priv(ie: IndefiniteEvent) -> TxReceipt {
    let insert_res = insert(ie.clone())
        .await
        .map(|tx_id| Nat::from(tx_id))
        .map_err(|_| TxError::Other);

    if insert_res.is_err() {
        TXLOG.with(|t| {
            let mut tx_log = t.borrow_mut();
            tx_log.ie_records.push_back(ie.clone());
        });
    }

    insert_res
}

/* MISC FNS */

#[pre_upgrade]
fn pre_upgrade() {
    let stats = STATS.with(|s| s.borrow().clone());
    let balances = BALANCES.with(|b| b.borrow().clone());
    let allows = ALLOWS.with(|a| a.borrow().clone());
    let blocks = BLOCKS.with(|b| b.borrow().clone());
    let tx_log = TXLOG.with(|t| t.borrow().clone());
    ic::stable_store((
        stats,
        balances,
        allows,
        blocks,
        tx_log,
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
    STATS.with(|s| {
        let mut stats = s.borrow_mut();
        *stats = metadata_stored;
    });
    BALANCES.with(|b| {
        let mut balances = b.borrow_mut();
        *balances = balances_stored;
    });
    ALLOWS.with(|a| {
        let mut allowances = a.borrow_mut();
        *allowances = allowances_stored;
    });
    BLOCKS.with(|b| {
        let mut blocks = b.borrow_mut();
        *blocks = blocks_stored;
    });
    TXLOG.with(|t| {
        let mut tx_log = t.borrow_mut();
        *tx_log = tx_log_stored;
    });
    CapEnv::load_from_archive(cap_env);
}

#[cfg(any(target_arch = "wasm32", test))]
fn main() {}

#[cfg(not(any(target_arch = "wasm32", test)))]
fn main() {
    candid::export_service!();
    std::print!("{}", __export_service());
}
