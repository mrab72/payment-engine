use std::collections::HashMap;
use serde::Deserialize;

use super::account::{Account, ClientId};
use super::transaction::{TxId, StoredTransaction};

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentsEngine {
    accounts: HashMap<ClientId, Account>,
    transactions: HashMap<TxId, StoredTransaction>,
}


impl Default for PaymentsEngine {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            transactions: HashMap::new(),
        }
    }
}

impl PaymentsEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieves an existing account or creates a new one if it doesn't exist.
    pub fn get_or_create_account(&mut self, client_id: ClientId) -> &mut Account {
        self.accounts.entry(client_id).or_insert_with(|| Account::new(client_id))
    }



}