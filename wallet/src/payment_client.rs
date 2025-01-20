pub trait PaymentClient {
    type PaymentCurrency;

    // TODO: Check whether amount of i32 can work or not
    async fn deposit(&self, user_id: i32, amount: i32) -> anyhow::Result<()>;

    async fn withdraw(&self, user_id: i32, amount: i32) -> anyhow::Result<()>;
}
