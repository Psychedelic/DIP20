/**
* Module     : main.rs
* Copyright  : 2022 Fleek
* License    : GPL 3.0
* Maintainer : Psychedelic <support@fleek.co>
* Stability  : Experimental
*/
use candid::{candid_method, CandidType, Deserialize, Int, Nat};
use cap_sdk::{handshake, insert, Event, IndefiniteEvent, TypedEvent};
use cap_std::dip20::cap::DIP20Details;
use cap_std::dip20::{Operation, TransactionStatus, TxRecord};
use ic_cdk_macros::*;
use ic_kit::{ic, Principal};
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
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
  Other(String),
}
pub type TxReceipt = Result<Nat, TxError>;

thread_local! {
    static BALANCES: RefCell<HashMap<Principal, Nat>> = RefCell::new(HashMap::default());
    static ALLOWS: RefCell<HashMap<Principal, HashMap<Principal, Nat>>> = RefCell::new(HashMap::default());
    static STATS: RefCell<StatsData> = RefCell::new(StatsData::default());
    static TXLOG: RefCell<TxLog> = RefCell::new(TxLog::default());
}

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
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.logo = logo;
    stats.name = name;
    stats.symbol = symbol;
    stats.decimals = decimals;
    stats.total_supply = total_supply.clone();
    stats.owner = owner;
    stats.fee = fee;
    stats.fee_to = fee_to;
    stats.history_size = 1;
    stats.deploy_time = ic::time();
  });
  handshake(1_000_000_000_000, Some(cap));
  BALANCES.with(|b| {
    b.borrow_mut().insert(owner, total_supply.clone());
  });
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

/* UPDATE FNS */

#[update]
#[candid_method(update)]
async fn transfer(to: Principal, value: Nat) -> TxReceipt {
  let from = ic::caller();
  let fee = _get_fee();
  if balance_of(from) < value.clone() + fee.clone() {
    return Err(TxError::InsufficientBalance);
  }
  _charge_fee(from, fee.clone());
  _transfer(from, to, value.clone());
  _history_inc();
  add_record(
    from,
    Operation::Transfer,
    from,
    to,
    value,
    fee,
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
  _charge_fee(from, fee.clone());
  _transfer(from, to, value.clone());
  ALLOWS.with(|a| {
    let mut allowances = a.borrow_mut();
    match allowances.get(&from) {
      Some(inner) => {
        let result = inner.get(&owner).unwrap().clone();
        let mut temp = inner.clone();
        if result.clone() - value.clone() - fee.clone() != 0 {
          temp.insert(owner, result.clone() - value.clone() - fee.clone());
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
    owner,
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

#[update]
#[candid_method(update)]
async fn approve(spender: Principal, value: Nat) -> TxReceipt {
  let owner = ic::caller();
  let fee = _get_fee();
  if balance_of(owner) < fee.clone() {
    return Err(TxError::InsufficientBalance);
  }
  _charge_fee(owner, fee.clone());
  let v = value.clone() + fee.clone();
  ALLOWS.with(|a| {
    let mut allowances = a.borrow_mut();
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
          allowances.insert(owner, inner);
        }
      }
    }
  });

  _history_inc();
  add_record(
    owner,
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

#[update]
#[candid_method(update)]
async fn burn(amount: Nat) -> TxReceipt {
  let caller = ic::caller();
  let caller_balance = balance_of(caller);
  if caller_balance.clone() < amount.clone() {
    return Err(TxError::InsufficientBalance);
  }
  BALANCES.with(|b| {
    let mut balances = b.borrow_mut();
    balances.insert(caller, caller_balance - amount.clone());
  });
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.total_supply -= amount.clone();
  });
  _history_inc();
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

/* QUERY FNS */

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

#[query]
#[candid_method(query)]
fn allowance(owner: Principal, spender: Principal) -> Nat {
  ALLOWS.with(|a| {
    let allowances = a.borrow();
    match allowances.get(&owner) {
      Some(inner) => match inner.get(&spender) {
        Some(value) => value.clone(),
        None => Nat::from(0),
      },
      None => Nat::from(0),
    }
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

#[query]
#[candid_method(query)]
fn name() -> String {
  STATS.with(|s| {
    let stats = s.borrow();
    stats.name.clone()
  })
}

#[query]
#[candid_method(query)]
fn symbol() -> String {
  STATS.with(|s| {
    let stats = s.borrow();
    stats.symbol.clone()
  })
}

#[query]
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

#[query]
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
  STATS.with(|stats| {
    let s = stats.borrow().clone();
    Metadata {
      logo: s.logo,
      name: s.name,
      symbol: s.symbol,
      decimals: s.decimals,
      totalSupply: s.total_supply,
      owner: s.owner,
      fee: s.fee,
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
      let balances = b.borrow();
      TokenInfo {
        metadata: get_metadata(),
        feeTo: stats.fee_to,
        historySize: stats.history_size,
        deployTime: stats.deploy_time,
        holderNumber: balances.len(),
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
    let mut balance = Vec::new();
    for (k, v) in balances.iter() {
      balance.push((k.clone(), v.clone()));
    }
    balance.sort_by(|a, b| b.1.cmp(&a.1));
    let limit: usize = if start + limit > balance.len() {
      balance.len() - start
    } else {
      limit
    };
    balance[start..start + limit].to_vec()
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
      Some(allow) => Vec::from_iter(allow.clone().into_iter()),
      None => Vec::new(),
    }
  })
}

/* CONTROLLER FNS */

#[update(guard = "_is_auth")]
#[candid_method(update, rename = "mint")]
async fn mint(to: Principal, amount: Nat) -> TxReceipt {
  let caller = ic::caller();
  let to_balance = balance_of(to);

  BALANCES.with(|b| {
    let mut balances = b.borrow_mut();
    balances.insert(to, to_balance + amount.clone());
  });
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.total_supply += amount.clone();
  });
  _history_inc();
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

#[update(name = "setName", guard = "_is_auth")]
#[candid_method(update, rename = "setName")]
fn set_name(name: String) {
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.name = name;
  });
}

#[update(name = "setLogo", guard = "_is_auth")]
#[candid_method(update, rename = "setLogo")]
fn set_logo(logo: String) {
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.logo = logo;
  });
}

#[update(name = "setFee", guard = "_is_auth")]
#[candid_method(update, rename = "setFee")]
fn set_fee(fee: Nat) {
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.fee = fee;
  });
}

#[update(name = "setFeeTo", guard = "_is_auth")]
#[candid_method(update, rename = "setFeeTo")]
fn set_fee_to(fee_to: Principal) {
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.fee_to = fee_to;
  });
}

#[update(name = "setOwner", guard = "_is_auth")]
#[candid_method(update, rename = "setOwner")]
fn set_owner(owner: Principal) {
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.owner = owner;
  });
}

/* INTERNAL FNS */

// TODO: use controllers for ownership
// this will require the canister to be a controller of itself (like dip721)
fn _is_auth() -> Result<(), String> {
  STATS.with(|s| {
    let stats = s.borrow();
    if ic::caller() == stats.owner {
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

fn _charge_fee(user: Principal, fee: Nat) {
  STATS.with(|s| {
    let stats = s.borrow();
    if stats.fee > Nat::from(0) {
      _transfer(user, stats.fee_to, fee);
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

fn _history_inc() {
  STATS.with(|s| {
    let mut stats = s.borrow_mut();
    stats.history_size += 1;
  })
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
  let stats = STATS.with(|s| s.borrow().clone());
  let balances = BALANCES.with(|b| b.borrow().clone());
  let allows = ALLOWS.with(|a| a.borrow().clone());
  let tx_log = TXLOG.with(|t| t.borrow().clone());
  ic::stable_store((stats, balances, allows, tx_log)).unwrap();
}

#[post_upgrade]
fn post_upgrade() {
  let (metadata_stored, balances_stored, allowances_stored, tx_log_stored): (
    StatsData,
    Balances,
    Allowances,
    TxLog,
  ) = ic::stable_restore().unwrap();
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
  TXLOG.with(|t| {
    let mut tx_log = t.borrow_mut();
    *tx_log = tx_log_stored;
  });
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
  let mut tx_log = TXLOG.with(|t| t.take());
  if let Some(failed_ie) = tx_log.ie_records.pop_front() {
    let _ = insert_into_cap_priv(failed_ie).await;
  }
  insert_into_cap_priv(ie).await
}

async fn insert_into_cap_priv(ie: IndefiniteEvent) -> TxReceipt {
  let insert_res = insert(ie.clone())
    .await
    .map(|tx_id| Nat::from(tx_id))
    .map_err(|error| TxError::Other(format!("Inserting into cap failed with error: {:?}", error)));

  if insert_res.is_err() {
    TXLOG.with(|t| {
      let mut tx_log = t.borrow_mut();
      tx_log.ie_records.push_back(ie.clone());
    });
  }

  insert_res
}
