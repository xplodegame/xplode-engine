use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CustomerDetails {
    /// Customer's name.
    pub name: String,
    /// The customer's phone number.
    /// TODO: Optional for now
    pub contact: Option<String>,
    /// The customer's email address.
    pub email: String,
}
