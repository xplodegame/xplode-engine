use std::env;

use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::models::User;

use super::models::CustomerDetails;

const RAZORPAY_API_URL: &str = "https://api.razorpay.com/v1";

#[derive(Serialize)]
pub struct CreateOrderRequest {
    /// The amount for which the order was created, in currency subunits.
    pub amount: u32, // Using f64 to accommodate both number and string types
    /// ISO code for the currency in which you want to accept the payment.
    pub currency: String,
    /// Receipt number that corresponds to this order, set for your internal reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,

    /// The payment method used to make the payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Details of the bank account that the customer has provided at the time of registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_details: Option<CustomerDetails>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateOrderResponse {
    pub id: String,
    pub currency: String,
    pub amount: u32,
    /// The payment method used to make the payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Details of the bank account that the customer has provided at the time of registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_details: Option<CustomerDetails>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyPaymentRequest {
    pub razorpay_payment_id: String,
    pub razorpay_order_id: String,
    pub razorpay_signature: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyPaymentResponse {
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct RazorpayClient {
    client: Client,
    api_key: String,
    api_secret: String,
}

impl RazorpayClient {
    pub fn new(api_key: String, api_secret: String) -> Self {
        let client = Client::new();
        RazorpayClient {
            client,
            api_key,
            api_secret,
        }
    }

    pub async fn create_order(
        &self,
        amount: u32,
        currency: &str,
        user: &User,
    ) -> Result<CreateOrderResponse, reqwest::Error> {
        println!("Creating order");
        let request = CreateOrderRequest {
            amount,
            currency: currency.to_string(),
            receipt: None,
            method: None,
            customer_details: Some(CustomerDetails {
                name: user.name.clone(),
                contact: None,
                email: user.email.clone(),
            }),
        };
        let response = self
            .client
            .post(format!("{}/orders", RAZORPAY_API_URL))
            .basic_auth(&self.api_key, Some(&self.api_secret))
            .json(&request)
            .send()
            .await?;

        response.json().await
    }

    pub async fn verify_payment(
        &self,
        razorpay_payment_id: &str,
        razorpay_order_id: &str,
        razorpay_signature: &str,
    ) -> Result<bool, reqwest::Error> {
        let sign = format!("{}|{}", razorpay_order_id, razorpay_payment_id);
        let secret = env::var("RAZORPAY_KEY_SECRET").unwrap();

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMac can take key of any size");

        mac.update(sign.as_bytes());

        let expected_sign = hex::encode(mac.finalize().into_bytes());

        if expected_sign == razorpay_signature {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
