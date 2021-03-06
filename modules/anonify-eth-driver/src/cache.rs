use crate::eth::event_watcher::PayloadType;
use log::{info, warn};
use parking_lot::RwLock;
use std::collections::hash_map::{Entry, HashMap};
use std::sync::Arc;
use web3::types::Address as ContractAddr;

type BlockNum = u64;
type RosterIdx = u32;
type Epoch = u32;
type Generation = u32;

// TODO: Prevent malicious TEE fraudulently setting the number of trials to break consistency.
pub const MAX_TRIALS_NUM: u32 = 50;

/// Cache data from events for arrival guarantee and order guarantee.
/// Unordered events are cached.
#[derive(Debug, Default, Clone)]
pub struct EventCache {
    inner: Arc<RwLock<InnerEventCache>>,
}

impl EventCache {
    pub fn inner(&self) -> &Arc<RwLock<InnerEventCache>> {
        &self.inner
    }
}

/// Do not implement `Clone` trait due to cache duplication.
#[derive(Debug, Default)]
pub struct InnerEventCache {
    block_num_counter: HashMap<ContractAddr, BlockNum>,
    treekem_counter: HashMap<RosterIdx, (Epoch, Generation)>,
    trials_counter: HashMap<RosterIdx, u32>,
    payloads_pool: HashMap<RosterIdx, Vec<PayloadType>>,
}

impl InnerEventCache {
    pub fn insert_next_block_num(
        &mut self,
        contract_addr: ContractAddr,
        block_num: BlockNum,
    ) -> Option<BlockNum> {
        info!("Insert: Cached block number: {}", block_num);
        self.block_num_counter.insert(contract_addr, block_num)
    }

    pub fn get_latest_block_num(&self, contract_addr: ContractAddr) -> Option<BlockNum> {
        let block_num = self.block_num_counter.get(&contract_addr).map(|e| *e);
        info!("Get: Cached block number: {:?}", block_num);
        block_num
    }

    /// In regard to order gurantee:
    /// There are two cases where the generation of received messages is not continuous.
    /// 1. In regard to the previous message, the sender's keychain ratcheted,
    ///    but some error occurred in the subsequent processing and it did not reach the receiver,
    ///    and its generation was skipped.
    /// 2. The order of the received messages is changed.
    ///    Due to the order guarantee between network connections and the transaction order,
    ///    the order is changed and recorded in the message queue (blockchain).
    /// In the case of 1, consistency is guaranteed between TEE node clusters,
    /// so there is no problem with processing as usual.
    /// In case of 2, it is necessary to process the message of
    /// the next generation received later without skipping first.
    /// Therefore, cache the message received earlier for a specific number of attempts shared by the cluster,
    /// and wait for the message of the next generation to come.
    /// If the next message does not come after waiting for the number of attempts, that message is skipped.
    /// (This skip process is performed by guaranteeing consistency in all TEEs in the cluster)
    pub fn ensure_order_guarantee(
        &mut self,
        mut payloads: Vec<PayloadType>,
        immutable_payloads: Vec<PayloadType>,
        max_trials_num: u32,
    ) -> Vec<PayloadType> {
        for (index, curr_payload) in immutable_payloads.iter().enumerate() {
            self.increment_trials_counter(&curr_payload);

            // The case current payload is the correct order
            if self.is_next_msg(&curr_payload) {
                self.update_treekem_counter(&curr_payload);
            } else {
                warn!("Received a discontinuous message: {:?}", curr_payload);

                let payloads_from_pool = self.find_next_payloads(&curr_payload, max_trials_num);

                // if reset the number of trials, skip the event
                if self
                    .trials_counter
                    .get(&curr_payload.roster_idx())
                    .unwrap_or_else(|| &0)
                    == &0
                {
                    self.update_treekem_counter(&curr_payload);
                    payloads = Self::insert_chunks_at_index(payloads, &payloads_from_pool, index);
                    return payloads;
                }

                // The case compensatable payloads are not found, so cache those.
                if payloads_from_pool.is_empty() {
                    warn!(
                        "Not found the next payload even in the cache, so cache the current payloads: {:?}",
                        curr_payload
                    );
                    self.insert_payloads_pool(curr_payload.clone(), &mut payloads);
                // The case compensatable payloads are found, so insert those to payloads buffer.
                } else {
                    if !payloads_from_pool[payloads_from_pool.len() - 1].is_next(&curr_payload) {
                        self.insert_payloads_pool(curr_payload.clone(), &mut payloads);
                    }
                    self.update_treekem_counter(&curr_payload);
                    payloads = Self::insert_chunks_at_index(payloads, &payloads_from_pool, index);
                }
            }
        }

        payloads
    }

    /// Increment the number of trials for each roster index
    pub fn increment_multi_trials_counter(&mut self, payloads: &[PayloadType]) {
        let mut roster_idx_list: Vec<RosterIdx> = payloads.iter().map(|p| p.roster_idx()).collect();
        roster_idx_list.dedup();
        for roster_idx in roster_idx_list {
            let traial_num = self.trials_counter.entry(roster_idx).or_default();
            *traial_num += 1;
        }
    }

    fn increment_trials_counter(&mut self, payload: &PayloadType) {
        let traial_num = self.trials_counter.entry(payload.roster_idx()).or_default();
        *traial_num += 1;
    }

    fn insert_chunks_at_index(
        mut payloads: Vec<PayloadType>,
        payloads_from_pool: &[PayloadType],
        index: usize,
    ) -> Vec<PayloadType> {
        payloads.reserve(payloads_from_pool.len());
        let mut v = payloads.split_off(index);
        payloads.extend_from_slice(&payloads_from_pool);
        payloads.append(&mut v);

        payloads
    }

    /// Whether the payload the next according to the treekem counter
    fn is_next_msg(&self, msg: &PayloadType) -> bool {
        let roster_idx = msg.roster_idx();
        let (current_epoch, current_gen) = *self
            .treekem_counter
            .get(&roster_idx)
            .unwrap_or_else(|| &(0, 0));

        if msg.epoch() == current_epoch {
            msg.generation() == current_gen + 1
                || msg.generation() == 0
                || msg.generation() == u32::MAX // handshake
        } else {
            // TODO: Handling reorder over epoch
            true
        }
    }

    /// Set the continuous payloads from cache pool into payloads buffer
    fn set_continuous_payloads(
        mut acc: Vec<PayloadType>,
        payloads_from_pool: &mut Vec<PayloadType>,
    ) -> Vec<PayloadType> {
        if !payloads_from_pool.is_empty() {
            let mut tmp = &payloads_from_pool[0];
            acc.push(tmp.clone());
            for curr_payload in &payloads_from_pool[1..] {
                if tmp.is_next(&curr_payload) {
                    acc.push(curr_payload.clone());
                    tmp = curr_payload;
                } else {
                    break;
                }
            }
            for _ in 0..acc.len() {
                payloads_from_pool.remove(0);
            }
        }

        acc
    }

    /// Finds next payloads.
    /// If the maximum number of trials is over,
    /// skip the event and get a continuous vector from the smallest payload in the `payloads_pool`,
    /// otherwise get continuous payloads from the `payloads_pool`.
    fn find_next_payloads(
        &mut self,
        prior_payload: &PayloadType,
        max_trials_num: u32,
    ) -> Vec<PayloadType> {
        let roster_idx = prior_payload.roster_idx();
        let mut acc: Vec<PayloadType> = vec![];

        let is_next_msg = match self.payloads_pool.get(&roster_idx) {
            Some(payloads) if !payloads.is_empty() => self.is_next_msg(&payloads[0]),
            _ => false,
        };

        match self.payloads_pool.entry(roster_idx) {
            Entry::Occupied(mut entry) => {
                if self.trials_counter.get(&roster_idx).unwrap_or_else(|| &0) > &max_trials_num {
                    acc = Self::set_continuous_payloads(acc, entry.get_mut());
                    warn!(
                        "The maximum number of trials is over, so skipped the next event of {:?}",
                        prior_payload
                    );
                    self.reset_trials_counter(roster_idx);
                } else {
                    if is_next_msg {
                        acc = Self::set_continuous_payloads(acc, entry.get_mut());
                        self.reset_trials_counter(roster_idx);
                    } else {
                        return acc;
                    }
                }
            }
            Entry::Vacant(_) => return acc,
        }

        acc
    }

    // reset the number of trials
    fn reset_trials_counter(&mut self, roster_idx: RosterIdx) {
        let trials_num = self.trials_counter.entry(roster_idx).or_default();
        *trials_num = 0;
    }

    fn insert_payloads_pool(&mut self, payload: PayloadType, payloads: &mut Vec<PayloadType>) {
        let payloads_from_pool = self.payloads_pool.entry(payload.roster_idx()).or_default();
        payloads_from_pool.push(payload.clone());
        payloads_from_pool.sort();

        // Duplicated items are already removed.
        payloads.remove(
            payloads
                .iter()
                .position(|p| p == &payload)
                .expect("payloads must have payload"),
        );
    }

    fn update_treekem_counter(&mut self, msg: &PayloadType) {
        self.treekem_counter
            .insert(msg.roster_idx(), (msg.epoch(), msg.generation()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correct_order_diff_roster_idx() {
        let dummy_payloads1 = vec![
            PayloadType::new(0, 0, 1, Default::default()),
            PayloadType::new(0, 0, 2, Default::default()),
            PayloadType::new(1, 0, 1, Default::default()),
            PayloadType::new(1, 0, 2, Default::default()),
            PayloadType::new(1, 0, 3, Default::default()),
        ];

        let dummy_payloads2 = vec![
            PayloadType::new(1, 0, 4, Default::default()),
            PayloadType::new(2, 0, 1, Default::default()),
        ];

        let mut cache = InnerEventCache::default();
        let res1 =
            cache.ensure_order_guarantee(dummy_payloads1.clone(), dummy_payloads1, MAX_TRIALS_NUM);
        assert_eq!(
            res1,
            vec![
                PayloadType::new(0, 0, 1, Default::default()),
                PayloadType::new(0, 0, 2, Default::default()),
                PayloadType::new(1, 0, 1, Default::default()),
                PayloadType::new(1, 0, 2, Default::default()),
                PayloadType::new(1, 0, 3, Default::default()),
            ]
        );

        let res2 =
            cache.ensure_order_guarantee(dummy_payloads2.clone(), dummy_payloads2, MAX_TRIALS_NUM);
        assert_eq!(
            res2,
            vec![
                PayloadType::new(1, 0, 4, Default::default()),
                PayloadType::new(2, 0, 1, Default::default()),
            ]
        );
    }

    #[test]
    fn test_fix_reorder_using_cache() {
        let dummy_payloads1 = vec![
            PayloadType::new(0, 0, 1, Default::default()),
            PayloadType::new(0, 0, 2, Default::default()),
            PayloadType::new(0, 0, 4, Default::default()),
            PayloadType::new(0, 0, 5, Default::default()),
        ];

        let dummy_payloads2 = vec![
            PayloadType::new(0, 0, 3, Default::default()),
            PayloadType::new(0, 0, 6, Default::default()),
            PayloadType::new(0, 0, 7, Default::default()),
        ];

        let mut cache = InnerEventCache::default();
        let res1 =
            cache.ensure_order_guarantee(dummy_payloads1.clone(), dummy_payloads1, MAX_TRIALS_NUM);
        assert_eq!(
            res1,
            vec![
                PayloadType::new(0, 0, 1, Default::default()),
                PayloadType::new(0, 0, 2, Default::default()),
            ]
        );

        let res2 =
            cache.ensure_order_guarantee(dummy_payloads2.clone(), dummy_payloads2, MAX_TRIALS_NUM);
        assert_eq!(
            res2,
            vec![
                PayloadType::new(0, 0, 3, Default::default()),
                PayloadType::new(0, 0, 4, Default::default()),
                PayloadType::new(0, 0, 5, Default::default()),
                PayloadType::new(0, 0, 6, Default::default()),
                PayloadType::new(0, 0, 7, Default::default()),
            ]
        );
    }

    #[test]
    fn test_fix_order_handshake() {
        let dummy_payloads1 = vec![
            PayloadType::new(0, 0, 1, Default::default()),
            PayloadType::new(0, 0, 2, Default::default()),
            PayloadType::new(0, 1, 1, Default::default()),
            PayloadType::new(0, 0, u32::MAX, Default::default()),
        ];

        let dummy_payloads2 = vec![
            PayloadType::new(0, 1, 2, Default::default()),
            PayloadType::new(0, 1, 3, Default::default()),
            PayloadType::new(0, 1, 4, Default::default()),
        ];

        let mut cache = InnerEventCache::default();
        let res1 =
            cache.ensure_order_guarantee(dummy_payloads1.clone(), dummy_payloads1, MAX_TRIALS_NUM);
        assert_eq!(
            res1,
            vec![
                PayloadType::new(0, 0, 1, Default::default()),
                PayloadType::new(0, 0, 2, Default::default()),
                PayloadType::new(0, 1, 1, Default::default()),
                PayloadType::new(0, 0, u32::MAX, Default::default()),
            ]
        );

        let res2 =
            cache.ensure_order_guarantee(dummy_payloads2.clone(), dummy_payloads2, MAX_TRIALS_NUM);
        assert_eq!(
            res2,
            vec![
                PayloadType::new(0, 1, 2, Default::default()),
                PayloadType::new(0, 1, 3, Default::default()),
                PayloadType::new(0, 1, 4, Default::default()),
            ]
        );
    }

    #[test]
    fn test_over_max_trials_num() {
        let dummy_payloads1 = vec![
            PayloadType::new(0, 0, 1, Default::default()),
            PayloadType::new(0, 0, 2, Default::default()),
            PayloadType::new(0, 0, 4, Default::default()),
            PayloadType::new(0, 0, 5, Default::default()),
        ];

        let dummy_payloads2 = vec![
            PayloadType::new(0, 0, 3, Default::default()),
            PayloadType::new(0, 0, 6, Default::default()),
            PayloadType::new(0, 0, 7, Default::default()),
        ];

        let mut cache = InnerEventCache::default();
        let res1 = cache.ensure_order_guarantee(dummy_payloads1.clone(), dummy_payloads1, 0);
        assert_eq!(
            res1,
            vec![
                PayloadType::new(0, 0, 1, Default::default()),
                PayloadType::new(0, 0, 2, Default::default()),
                PayloadType::new(0, 0, 5, Default::default()),
                PayloadType::new(0, 0, 4, Default::default()),
            ]
        );

        let res2 = cache.ensure_order_guarantee(dummy_payloads2.clone(), dummy_payloads2, 0);
        assert_eq!(
            res2,
            vec![
                PayloadType::new(0, 0, 3, Default::default()),
                PayloadType::new(0, 0, 6, Default::default()),
                PayloadType::new(0, 0, 7, Default::default()),
            ]
        );
    }
}
