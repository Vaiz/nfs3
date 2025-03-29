use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// `TransactionTracker` tracks the state of transactions to detect retransmissions.
pub struct TransactionTracker {
    retention_period: Duration,
    transactions: RwLock<HashMap<String, Arc<Mutex<ClientTransactions>>>>,
}

impl TransactionTracker {
    pub fn new(retention_period: Duration) -> Self {
        Self {
            retention_period,
            transactions: RwLock::new(HashMap::new()),
        }
    }

    #[must_use]
    pub(crate) fn start_transaction(
        &self,
        client_addr: &str,
        xid: u32,
        now: Instant,
    ) -> Option<TransactionLock> {
        // First, we check if client is already in the transactions map
        {
            let transactions = self
                .transactions
                .read()
                .expect("unable to unlock transactions mutex");

            if let Some(client_transactions) = transactions.get(client_addr) {
                let mut client_lock = client_transactions.lock().unwrap();
                client_lock.add_transaction(xid, now).ok()?;
                return Some(TransactionLock::new(
                    client_transactions.clone(),
                    xid,
                    self.retention_period,
                ));
            }
        }

        // If client is not in the transactions map, we need to add it
        // It's possible that another thread added it while we were checking, so we need to
        // check again
        let mut transactions = self
            .transactions
            .write()
            .expect("unable to unlock transactions mutex");

        let val = transactions
            .entry(client_addr.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(ClientTransactions::new(now))));

        let mut client_lock = val.lock().unwrap();
        client_lock.add_transaction(xid, now).ok()?;

        Some(TransactionLock::new(
            val.clone(),
            xid,
            self.retention_period,
        ))
    }

    pub(crate) fn cleanup(&self, now: Instant) {
        let mut transactions = self
            .transactions
            .write()
            .expect("unable to unlock transactions mutex");

        transactions.retain(|_, client_transactions| {
            let mut client_lock = client_transactions.lock().unwrap();
            if client_lock.is_active(now, self.retention_period) {
                client_lock.remove_old_transactions(now, self.retention_period);
                true
            } else {
                // If the client is not active, we remove it from the map
                false
            }
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionState {
    InProgress,
    Completed(Instant),
}

#[derive(Debug)]
struct Transaction {
    xid: u32,
    state: TransactionState,
}

impl Transaction {
    fn in_progress(xid: u32) -> Self {
        Self {
            xid,
            state: TransactionState::InProgress,
        }
    }

    fn complete(&mut self, now: Instant) {
        self.state = TransactionState::Completed(now);
    }

    fn is_stale(&self, now: Instant, max_age: Duration) -> bool {
        match self.state {
            TransactionState::InProgress => false,
            TransactionState::Completed(tx_time) => now - tx_time > max_age,
        }
    }
}

#[derive(Debug)]
struct ClientTransactions {
    // Sorted by the xid of the transaction
    // In general, it's expected that transactions from the same host will be in order
    transactions: VecDeque<Transaction>,
    last_active: Instant,
}

impl ClientTransactions {
    fn new(now: Instant) -> Self {
        Self {
            transactions: VecDeque::new(),
            last_active: now,
        }
    }
    // Finds a transaction by its xid
    // First, it checks last transaction in the list
    // If first transaction has different xid, it does a binary search to find the transaction
    fn find_transaction(&self, xid: u32) -> Result<usize, usize> {
        use std::cmp::Ordering;
        if let Some(last_tx) = self.transactions.back() {
            match last_tx.xid.cmp(&xid) {
                // transaction is the last one, so we can return it directly
                Ordering::Equal => Ok(self.transactions.len() - 1),
                // transaction does not exist, so we return the position where it should be inserted
                Ordering::Less => Err(self.transactions.len()),
                // transaction is not the last one, so we need to do a binary search
                Ordering::Greater => self.transactions.binary_search_by_key(&xid, |t| t.xid),
            }
        } else {
            // transaction list is empty
            Err(0)
        }
    }

    fn add_transaction(&mut self, xid: u32, now: Instant) -> Result<(), &Transaction> {
        self.last_active = now;
        match self.find_transaction(xid) {
            Ok(p) => {
                // transaction already exists
                Err(&self.transactions[p])
            }
            Err(p) => {
                self.transactions.insert(p, Transaction::in_progress(xid));
                Ok(())
            }
        }
    }

    fn complete_transaction(&mut self, xid: u32, now: Instant) {
        self.last_active = now;
        if let Ok(p) = self.find_transaction(xid) {
            self.transactions[p].complete(now);
        } else {
            // transaction not found, do nothing
        }
    }

    /// Removes transactions older than the specified max_age, starting from the beginning of the
    /// list.
    fn remove_old_transactions(&mut self, now: Instant, max_age: Duration) {
        while let Some(tx) = self.transactions.front() {
            if tx.is_stale(now, max_age) {
                self.transactions.pop_front();
            } else {
                break;
            }
        }
    }

    fn is_active(&self, now: Instant, max_age: Duration) -> bool {
        if now - self.last_active < max_age {
            true
        } else {
            self.has_active_transactions(now, max_age)
        }
    }

    fn has_active_transactions(&self, now: Instant, max_age: Duration) -> bool {
        self.transactions
            .iter()
            .any(|tx| !tx.is_stale(now, max_age))
    }
}

#[derive(Debug)]
pub(crate) struct TransactionLock {
    transactions: Arc<Mutex<ClientTransactions>>,
    xid: u32,
    retention_period: Duration,
}

impl TransactionLock {
    fn new(
        transactions: Arc<Mutex<ClientTransactions>>,
        xid: u32,
        retention_period: Duration,
    ) -> Self {
        Self {
            transactions,
            xid,
            retention_period,
        }
    }
}

impl Drop for TransactionLock {
    fn drop(&mut self) {
        let now = Instant::now();
        let mut transactions = self.transactions.lock().unwrap();
        transactions.complete_transaction(self.xid, now);
        transactions.remove_old_transactions(now, self.retention_period);
    }
}

pub struct Cleaner {
    tracker: Arc<TransactionTracker>,
    interval: Duration,
    stop: Arc<tokio::sync::Notify>,
}

impl Cleaner {
    pub fn new(
        tracker: Arc<TransactionTracker>,
        interval: Duration,
        stop: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            tracker,
            interval,
            stop,
        }
    }

    pub async fn run(self) {
        tracing::debug!("Transaction tracker cleaner started");
        loop {
            tokio::select! {
                _ = self.stop.notified() => break,
                _ = tokio::time::sleep(self.interval) => {
                    self.tracker.cleanup(Instant::now());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction() {
        let mut transaction = Transaction::in_progress(1);
        assert_eq!(transaction.xid, 1);
        assert!(matches!(transaction.state, TransactionState::InProgress));

        let now = Instant::now();
        transaction.complete(now);
        assert!(matches!(transaction.state, TransactionState::Completed(_)));

        let max_age = Duration::new(1, 0);
        assert!(!transaction.is_stale(now, max_age));
        assert!(transaction.is_stale(now + max_age + Duration::new(1, 0), max_age));
    }

    #[test]
    fn test_client_transactions() {
        let now = Instant::now();
        let mut client_transactions = ClientTransactions::new(now);

        assert_eq!(client_transactions.transactions.len(), 0);
        assert!(client_transactions.last_active.elapsed() < Duration::new(1, 0));

        client_transactions.add_transaction(1, now).unwrap();
        assert_eq!(client_transactions.transactions.len(), 1);
        assert_eq!(client_transactions.transactions[0].xid, 1);

        client_transactions.complete_transaction(1, now);
        assert_eq!(
            client_transactions.transactions[0].state,
            TransactionState::Completed(now)
        );

        client_transactions.remove_old_transactions(now + Duration::new(2, 0), Duration::new(1, 0));
        assert_eq!(client_transactions.transactions.len(), 0);
    }

    #[test]
    fn test_client_transactions_stale() {
        let now = Instant::now();
        let mut client_transactions = ClientTransactions::new(now);

        client_transactions.add_transaction(1, now).unwrap();
        client_transactions.add_transaction(2, now).unwrap();
        client_transactions.complete_transaction(2, now);

        assert_eq!(client_transactions.transactions.len(), 2);
        assert_eq!(client_transactions.transactions[0].xid, 1);
        assert_eq!(client_transactions.transactions[1].xid, 2);
        assert!(client_transactions.transactions[0].state == TransactionState::InProgress);
        assert!(client_transactions.transactions[1].state == TransactionState::Completed(now));

        client_transactions.remove_old_transactions(now + Duration::new(2, 0), Duration::new(1, 0));
        assert_eq!(client_transactions.transactions.len(), 2);

        client_transactions.complete_transaction(1, now);
        assert_eq!(
            client_transactions.transactions[0].state,
            TransactionState::Completed(now)
        );
        assert_eq!(
            client_transactions.transactions[1].state,
            TransactionState::Completed(now)
        );
        client_transactions.remove_old_transactions(now + Duration::new(2, 0), Duration::new(1, 0));

        assert_eq!(client_transactions.transactions.len(), 0);
    }

    #[test]
    fn test_transaction_tracker() -> anyhow::Result<()> {
        let tracker = TransactionTracker::new(Duration::new(1, 0));
        let now = Instant::now();

        let transaction = tracker.start_transaction("client1", 1, now).unwrap();
        assert!(tracker.start_transaction("client1", 1, now).is_none());
        assert_eq!(transaction.xid, 1);

        {
            let tracker_lock = tracker.transactions.read().unwrap();
            assert_eq!(tracker_lock.len(), 1);
            let client = tracker_lock.get("client1").unwrap();
            let client = client.lock().unwrap();
            assert_eq!(client.transactions.len(), 1);
            assert_eq!(client.transactions[0].xid, 1);
            assert_eq!(client.last_active, now);
            assert_eq!(client.transactions[0].state, TransactionState::InProgress);
        }

        drop(transaction);

        {
            let tracker_lock = tracker.transactions.read().unwrap();
            assert_eq!(tracker_lock.len(), 1);
            let client = tracker_lock.get("client1").unwrap();
            let client = client.lock().unwrap();
            assert_eq!(client.transactions.len(), 1);
            assert_eq!(client.transactions[0].xid, 1);
            assert!(client.last_active >= now);
            assert!(matches!(
                client.transactions[0].state,
                TransactionState::Completed(_)
            ));
        }

        Ok(())
    }

    #[test]
    fn test_cleanup() {
        let tracker = TransactionTracker::new(Duration::new(1, 0));
        let now = Instant::now();

        let transaction1 = tracker.start_transaction("client1", 1, now).unwrap();
        let transaction2 = tracker.start_transaction("client1", 2, now).unwrap();

        tracker.cleanup(now + Duration::new(2, 0));

        {
            let tracker_lock = tracker.transactions.read().unwrap();
            assert_eq!(tracker_lock.len(), 1);
            let client = tracker_lock.get("client1").unwrap();
            let client = client.lock().unwrap();
            assert_eq!(client.transactions.len(), 2);
        }

        tracker.cleanup(now + Duration::new(3, 0));

        {
            let tracker_lock = tracker.transactions.read().unwrap();
            assert_eq!(tracker_lock.len(), 1);
        }

        drop(transaction1);
        let now = Instant::now(); // drop updates the time

        tracker.cleanup(now + Duration::new(4, 0));

        {
            let tracker_lock = tracker.transactions.read().unwrap();
            assert_eq!(tracker_lock.len(), 1);
            let client = tracker_lock.get("client1").unwrap();
            let client = client.lock().unwrap();
            assert_eq!(client.transactions.len(), 1);
        }
        drop(transaction2);
        let now = Instant::now(); // drop updates the time

        tracker.cleanup(now + Duration::new(5, 0));

        {
            let tracker_lock = tracker.transactions.read().unwrap();
            assert_eq!(tracker_lock.len(), 0);
        }
    }
}
