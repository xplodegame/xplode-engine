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

impl_from_str_for_enum!(Currency, INR, SOL, USDC);
impl_to_string_for_enum!(Currency, INR, SOL, USDC);
impl_from_str_for_enum!(TxType, DEPOSIT, WITHDRAWAL);
impl_to_string_for_enum!(TxType, DEPOSIT, WITHDRAWAL);
