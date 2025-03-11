use serde::{Deserialize, Serialize};

use crate::{impl_from_str_for_enum, impl_to_string_for_enum};

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Currency {
    INR,
    SOL,
    USDC,
    MON,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TxType {
    DEPOSIT,
    WITHDRAWAL,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Network {
    SOLANA,
    MONAD,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum WalletType {
    PDA,
    DIRECT,
}

#[derive(Deserialize)]
pub struct UserDetailsRequest {
    pub name: String,
    pub email: String,
    pub clerk_id: String,
    pub wallet_address: Option<String>,
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

impl_from_str_for_enum!(Currency, INR, SOL, USDC, MON);
impl_to_string_for_enum!(Currency, INR, SOL, USDC, MON);
impl_from_str_for_enum!(TxType, DEPOSIT, WITHDRAWAL);
impl_to_string_for_enum!(TxType, DEPOSIT, WITHDRAWAL);
impl_from_str_for_enum!(Network, SOLANA, MONAD);
impl_to_string_for_enum!(Network, SOLANA, MONAD);
impl_from_str_for_enum!(WalletType, PDA, DIRECT);
impl_to_string_for_enum!(WalletType, PDA, DIRECT);
