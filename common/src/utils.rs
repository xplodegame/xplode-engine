use serde::{Deserialize, Serialize};

use crate::{impl_from_str_for_enum, impl_to_string_for_enum};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
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
    MINT,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum Network {
    SOLANA,
    MONAD,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum WalletType {
    PDA,
    DIRECT,
}

#[derive(Deserialize, Debug)]
pub struct UserDetailsRequest {
    pub name: String,
    pub email: String,
    pub privy_id: String,
    pub wallet_address: Option<String>,
    pub currency: Option<Currency>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserDetailsResponse {
    pub id: i32,
    pub name: String,
    pub email: String,
    pub balance: f64,
    pub privy_id: String,
    pub wallet_address: Option<String>,
    pub currency: Option<Currency>,
    pub gif_ids: Vec<i32>,
}

#[derive(Deserialize, Debug)]
pub struct UpdateUserDetailsRequest {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct DepositRequest {
    pub user_id: i32,
    pub amount: f64,
    pub currency: Currency,
    pub tx_type: TxType,
    pub tx_hash: String,
}

#[derive(Deserialize, Debug)]
pub struct WithdrawRequest {
    pub user_id: i32,
    pub amount: f64,
    pub currency: Currency,
    pub withdraw_address: String,
}

#[derive(Deserialize, Debug)]
pub struct MintNftRequest {
    pub user_id: i32,
    pub gif_id: i32,
    pub mint_amount: f64,
    pub currency: Currency,
    pub tx_hash: String,
}

impl_from_str_for_enum!(Currency, INR, SOL, USDC, MON);
impl_to_string_for_enum!(Currency, INR, SOL, USDC, MON);
impl_from_str_for_enum!(TxType, DEPOSIT, WITHDRAWAL, MINT);
impl_to_string_for_enum!(TxType, DEPOSIT, WITHDRAWAL, MINT);
impl_from_str_for_enum!(Network, SOLANA, MONAD);
impl_to_string_for_enum!(Network, SOLANA, MONAD);
impl_from_str_for_enum!(WalletType, PDA, DIRECT);
impl_to_string_for_enum!(WalletType, PDA, DIRECT);
