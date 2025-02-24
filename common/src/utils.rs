use serde::{Deserialize, Serialize};

use crate::{impl_from_str_for_enum, impl_to_string_for_enum};

#[derive(Debug, Serialize, Deserialize)]
pub enum Currency {
    INR,
    SOL,
    USDC,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TxType {
    DEPOSIT,
    WITHDRAWAL,
}

#[derive(Deserialize)]
pub struct UserDetailsRequest {
    pub name: String,
    pub email: String,
    pub clerk_id: String,
}

#[derive(Deserialize)]
pub struct DepositRequest {
    pub user_id: i32,
    pub amount: f64,
    pub currency: Currency,
    pub tx_type: TxType,
    pub tx_hash: String,
}

#[derive(Deserialize)]
pub struct WithdrawRequest {
    pub user_id: i32,
    pub amount: f64,
    pub currency: Currency,
    pub withdraw_address: String,
}

impl_from_str_for_enum!(Currency, INR, SOL, USDC);
impl_to_string_for_enum!(Currency, INR, SOL, USDC);
impl_from_str_for_enum!(TxType, DEPOSIT, WITHDRAWAL);
impl_to_string_for_enum!(TxType, DEPOSIT, WITHDRAWAL);
