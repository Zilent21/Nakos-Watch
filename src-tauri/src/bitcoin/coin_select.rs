//! Coin selection policies for the Spending Planner. Pure functions, no I/O.
//!
//! Fee vs Privacy are deliberately different code paths:
//!
//! * `Fee`: fewest inputs, hence fewest vbytes. Spends the largest UTXOs
//!   first (largest-first is count-minimal); a single largest coin that
//!   covers the amount is used alone.
//! * `Privacy`: (1) restrict to one script type — the type with the most
//!   coins that can cover the target on its own — so the tx doesn't mix
//!   address types; only if no single type suffices are types mixed.
//!   (2) Don't peel the biggest pile: the largest coin of that pool is
//!   excluded when the rest still covers the target. (3) Use the smallest
//!   single coin that covers the target if there is one (no input linking),
//!   otherwise take the pool's coins largest-first until covered.
//!   Not a mixer; just heuristics that reduce obvious linkage.
//!
//! Fragmentation (`select_smallest_first`): the smallest coins first, to clear dust, taking coins
//! until the amount plus the fee they add is covered. Like every other policy it accounts for the
//! fee, so it keeps going past the point where the coins merely equal the amount.
//!
//! Tax (`select_hifo`): HIFO (highest-in, first-out) over coins that have a
//! recorded cost-basis lot. Coins are ordered by cost per sat, highest first,
//! so the spend realises the smallest gain (or largest loss); ties go to the
//! older acquisition date, then id. Whole coins only: it does not model
//! holding periods, specific-lot elections or wash-sale rules, and is not
//! tax advice. A coin with no lot is never selected.
//!
//! Fee needed is estimated from per-script input sizes at the given rate, so
//! the selected set covers `amount + fee`. If nothing covers it, `None`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub amount_sats: u64,
    pub script: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Fee,
    Privacy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub ids: Vec<String>,
    pub total_sats: u64,
    /// True when every selected input has the same script type.
    pub single_script_type: bool,
    pub script_types: Vec<&'static str>,
}

/// Classify by address encoding. Heuristic only (no script on the UTXO row).
pub fn script_class(address: &str) -> &'static str {
    let a = address.to_ascii_lowercase();
    if a.starts_with("bc1p") || a.starts_with("tb1p") || a.starts_with("bcrt1p") {
        "p2tr"
    } else if a.starts_with("bc1q") || a.starts_with("tb1q") || a.starts_with("bcrt1q") {
        // P2WPKH is 42 chars (bc1q) / 44 (tb1q); longer means P2WSH.
        if a.len() <= 44 { "p2wpkh" } else { "p2wsh" }
    } else if a.starts_with('3') || a.starts_with('2') {
        "p2sh"
    } else if a.starts_with('1') || a.starts_with('m') || a.starts_with('n') {
        "p2pkh"
    } else {
        "unknown"
    }
}

fn input_vbytes(script: &str) -> f64 {
    match script {
        "p2tr" => 57.5,
        "p2wpkh" => 68.0,
        "p2wsh" => 105.0,
        "p2sh" => 91.0,
        "p2pkh" => 148.0,
        _ => 91.0,
    }
}

/// Fee estimate for spending `set` to one recipient plus change.
fn fee_for(set: &[&Candidate], fee_rate: f64) -> u64 {
    let vb = 10.5 + 2.0 * 31.0 + set.iter().map(|c| input_vbytes(c.script)).sum::<f64>();
    (vb * fee_rate).ceil() as u64
}

fn covers(set: &[&Candidate], amount: u64, fee_rate: f64) -> bool {
    set.iter().map(|c| c.amount_sats).sum::<u64>() >= amount + fee_for(set, fee_rate)
}

/// Take from `sorted_desc` in order until `amount + fee` is covered.
fn accumulate<'a>(sorted_desc: &[&'a Candidate], amount: u64, fee_rate: f64) -> Option<Vec<&'a Candidate>> {
    let mut picked: Vec<&Candidate> = Vec::new();
    for c in sorted_desc {
        picked.push(c);
        if covers(&picked, amount, fee_rate) {
            return Some(picked);
        }
    }
    None
}

fn sorted_desc(cands: &[Candidate]) -> Vec<&Candidate> {
    let mut v: Vec<&Candidate> = cands.iter().collect();
    // id tiebreak keeps selection deterministic.
    v.sort_by(|a, b| b.amount_sats.cmp(&a.amount_sats).then_with(|| a.id.cmp(&b.id)));
    v
}

pub fn select(cands: &[Candidate], amount: u64, fee_rate: f64, policy: Policy) -> Option<Selection> {
    let picked = match policy {
        Policy::Fee => select_fee(cands, amount, fee_rate)?,
        Policy::Privacy => select_privacy(cands, amount, fee_rate)?,
    };
    Some(selection_of(&picked))
}

fn selection_of(picked: &[&Candidate]) -> Selection {
    let mut types: Vec<&'static str> = picked.iter().map(|c| c.script).collect();
    types.sort_unstable();
    types.dedup();
    Selection {
        ids: picked.iter().map(|c| c.id.clone()).collect(),
        total_sats: picked.iter().map(|c| c.amount_sats).sum(),
        single_script_type: types.len() == 1,
        script_types: types,
    }
}

/// Fragmentation: smallest coins first (ties by id, so it is deterministic), taking coins until
/// `amount + fee` is covered. `None` when even all of them together cannot cover it.
pub fn select_smallest_first(cands: &[Candidate], amount: u64, fee_rate: f64) -> Option<Selection> {
    let mut asc: Vec<&Candidate> = cands.iter().collect();
    asc.sort_by(|a, b| a.amount_sats.cmp(&b.amount_sats).then_with(|| a.id.cmp(&b.id)));
    accumulate(&asc, amount, fee_rate).map(|picked| selection_of(&picked))
}

fn select_fee(cands: &[Candidate], amount: u64, fee_rate: f64) -> Option<Vec<&Candidate>> {
    accumulate(&sorted_desc(cands), amount, fee_rate)
}

fn select_privacy(cands: &[Candidate], amount: u64, fee_rate: f64) -> Option<Vec<&Candidate>> {
    // 1. Pick the script-type pool: most coins among types that cover alone
    //    (ties: larger total, then name); fall back to everything.
    let mut types: Vec<&'static str> = cands.iter().map(|c| c.script).collect();
    types.sort_unstable();
    types.dedup();
    let pool_for = |t: &str| -> Vec<&Candidate> { sorted_desc(cands).into_iter().filter(|c| c.script == t).collect() };
    let best = types
        .iter()
        .map(|t| (*t, pool_for(t)))
        .filter(|(_, p)| covers(p, amount, fee_rate))
        .max_by(|(ta, a), (tb, b)| {
            a.len()
                .cmp(&b.len())
                .then_with(|| a.iter().map(|c| c.amount_sats).sum::<u64>().cmp(&b.iter().map(|c| c.amount_sats).sum::<u64>()))
                .then_with(|| tb.cmp(ta))
        })
        .map(|(_, p)| p);
    let pool = best.unwrap_or_else(|| sorted_desc(cands));

    // 2. Don't peel the biggest pile if the rest can pay.
    let rest: &[&Candidate] = if pool.len() > 1 && covers(&pool[1..], amount, fee_rate) { &pool[1..] } else { &pool };

    // 3. Smallest single coin that covers; else largest-first within `rest`.
    if let Some(single) = rest.iter().rev().find(|c| covers(&[**c], amount, fee_rate)) {
        return Some(vec![*single]);
    }
    accumulate(rest, amount, fee_rate)
}

/// A recorded cost-basis lot for one coin (a cost and the date acquired).
#[derive(Debug, Clone, PartialEq)]
pub struct Lot {
    pub id: String,
    pub cost_fiat: f64,
    /// YYYY-MM-DD
    pub acquired: String,
}

/// HIFO selection over coins that have a lot. Errors (never guesses) when no
/// coin has a lot, or when the coins that do can't cover amount plus fee.
pub fn select_hifo(cands: &[Candidate], lots: &[Lot], amount: u64, fee_rate: f64) -> Result<Selection, String> {
    let with_lot: Vec<(&Candidate, &Lot)> = cands.iter().filter_map(|c| lots.iter().find(|l| l.id == c.id).map(|l| (c, l))).collect();
    let missing = cands.len() - with_lot.len();
    if with_lot.is_empty() {
        return Err(format!(
            "\"Lowest tax impact\" needs cost basis, and none of this wallet's {} spendable coin(s) has one. Add it in UTXO Manager.",
            cands.len()
        ));
    }
    let unit_cost = |c: &Candidate, l: &Lot| if c.amount_sats == 0 { 0.0 } else { l.cost_fiat / c.amount_sats as f64 };
    let mut ordered = with_lot.clone();
    ordered.sort_by(|(ca, la), (cb, lb)| {
        unit_cost(cb, lb)
            .partial_cmp(&unit_cost(ca, la))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| la.acquired.cmp(&lb.acquired))
            .then_with(|| ca.id.cmp(&cb.id))
    });
    let ordered_refs: Vec<&Candidate> = ordered.iter().map(|(c, _)| *c).collect();
    let picked = accumulate(&ordered_refs, amount, fee_rate).ok_or_else(|| {
        format!(
            "the {} coin(s) with cost basis can't cover this amount plus fee; {missing} coin(s) have no basis recorded. Add basis in UTXO Manager or lower the amount.",
            with_lot.len()
        )
    })?;
    let mut types: Vec<&'static str> = picked.iter().map(|c| c.script).collect();
    types.sort_unstable();
    types.dedup();
    Ok(Selection {
        ids: picked.iter().map(|c| c.id.clone()).collect(),
        total_sats: picked.iter().map(|c| c.amount_sats).sum(),
        single_script_type: types.len() == 1,
        script_types: types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, sats: u64, script: &'static str) -> Candidate {
        Candidate { id: id.into(), amount_sats: sats, script }
    }

    fn ids(s: &Selection) -> Vec<&str> {
        s.ids.iter().map(String::as_str).collect()
    }

    #[test]
    fn fee_and_privacy_pick_different_sets_same_target() {
        let u = vec![c("a", 100_000, "p2wpkh"), c("b", 60_000, "p2wpkh"), c("c", 40_000, "p2wpkh"), c("d", 20_000, "p2wpkh")];
        let fee = select(&u, 30_000, 5.0, Policy::Fee).unwrap();
        let priv_ = select(&u, 30_000, 5.0, Policy::Privacy).unwrap();
        assert_eq!(ids(&fee), ["a"]);
        assert_eq!(ids(&priv_), ["c"]); // smallest sufficient, biggest not peeled
        assert_ne!(fee.ids, priv_.ids);
    }

    #[test]
    fn privacy_keeps_one_script_type_when_possible() {
        let u = vec![
            c("w1", 70_000, "p2wpkh"),
            c("w2", 50_000, "p2wpkh"),
            c("w3", 45_000, "p2wpkh"),
            c("t1", 200_000, "p2tr"),
        ];
        let fee = select(&u, 100_000, 5.0, Policy::Fee).unwrap();
        let priv_ = select(&u, 100_000, 5.0, Policy::Privacy).unwrap();
        assert_eq!(ids(&fee), ["t1"]);
        assert_eq!(ids(&priv_), ["w1", "w2"]);
        assert!(priv_.single_script_type);
    }

    #[test]
    fn privacy_mixes_types_only_when_forced() {
        let u = vec![c("w", 60_000, "p2wpkh"), c("t", 60_000, "p2tr")];
        let s = select(&u, 100_000, 5.0, Policy::Privacy).unwrap();
        assert_eq!(s.ids.len(), 2);
        assert!(!s.single_script_type);
    }

    #[test]
    fn single_utxo_is_identical_for_both_modes() {
        let u = vec![c("only", 500_000, "p2wpkh")];
        assert_eq!(select(&u, 100_000, 5.0, Policy::Fee), select(&u, 100_000, 5.0, Policy::Privacy));
    }

    #[test]
    fn insufficient_after_fee_returns_none() {
        let u = vec![c("a", 10_000, "p2wpkh")];
        assert!(select(&u, 10_000, 5.0, Policy::Fee).is_none());
        assert!(select(&u, 10_000, 5.0, Policy::Privacy).is_none());
    }

    #[test]
    fn script_class_by_address() {
        assert_eq!(script_class("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"), "p2wpkh");
        assert_eq!(script_class("bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr"), "p2tr");
        assert_eq!(script_class("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy"), "p2sh");
        assert_eq!(script_class("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2"), "p2pkh");
    }

    // ---- HIFO ---------------------------------------------------------------

    fn lot(id: &str, cost: f64, date: &str) -> Lot {
        Lot { id: id.into(), cost_fiat: cost, acquired: date.into() }
    }

    #[test]
    fn missing_basis_rejects_tax_selection() {
        let u = vec![c("a", 100_000, "p2wpkh"), c("b", 50_000, "p2wpkh")];
        let err = select_hifo(&u, &[], 30_000, 5.0).unwrap_err();
        assert!(err.contains("needs cost basis"), "{err}");
    }

    #[test]
    fn hifo_spends_the_highest_cost_per_sat_first() {
        // a: $10 for 100k sats ($0.0001/sat), b: $30 for 100k ($0.0003/sat), c: $5 for 100k.
        let u = vec![c("a", 100_000, "p2wpkh"), c("b", 100_000, "p2wpkh"), c("c", 100_000, "p2wpkh")];
        let lots = vec![lot("a", 10.0, "2023-01-01"), lot("b", 30.0, "2024-06-01"), lot("c", 5.0, "2020-01-01")];
        let s = select_hifo(&u, &lots, 50_000, 5.0).unwrap();
        assert_eq!(s.ids, vec!["b"], "highest basis per sat realises the least gain");
        // FIFO would have picked the oldest ("c"); this is deliberately different.
        let big = select_hifo(&u, &lots, 150_000, 5.0).unwrap();
        assert_eq!(big.ids, vec!["b", "a"]);
    }

    #[test]
    fn hifo_ties_go_to_the_older_lot_then_id() {
        let u = vec![c("x", 100_000, "p2wpkh"), c("y", 100_000, "p2wpkh")];
        let lots = vec![lot("x", 10.0, "2024-01-01"), lot("y", 10.0, "2021-01-01")];
        assert_eq!(select_hifo(&u, &lots, 10_000, 5.0).unwrap().ids, vec!["y"]);
    }

    #[test]
    fn coins_without_a_lot_are_never_used() {
        // "big" has no lot and would otherwise be needed; the lot-backed coins can't cover it.
        let u = vec![c("big", 1_000_000, "p2wpkh"), c("small", 20_000, "p2wpkh")];
        let lots = vec![lot("small", 5.0, "2024-01-01")];
        let err = select_hifo(&u, &lots, 500_000, 5.0).unwrap_err();
        assert!(err.contains("1 coin(s) have no basis recorded"), "{err}");
        // A small spend is fine and only touches the lot-backed coin.
        assert_eq!(select_hifo(&u, &lots, 5_000, 5.0).unwrap().ids, vec!["small"]);
    }

    // ---- Fragmentation (R10): smallest first, but the fee is part of "covered" ----

    #[test]
    fn fragmentation_keeps_going_when_the_coins_only_equal_the_amount() {
        // 1_000 + 2_000 equals the 3_000 amount, but two inputs cost ~2_085 sats at 10 sat/vB.
        let u = vec![c("a", 1_000, "p2wpkh"), c("b", 2_000, "p2wpkh"), c("c", 10_000, "p2wpkh")];
        let s = select_smallest_first(&u, 3_000, 10.0).unwrap();
        assert_eq!(ids(&s), ["a", "b", "c"]);
        let refs: Vec<&Candidate> = u.iter().collect();
        assert!(s.total_sats >= 3_000 + fee_for(&refs, 10.0));
    }

    #[test]
    fn fragmentation_takes_the_smallest_first_with_a_stable_tiebreak_and_stops_when_covered() {
        let u = vec![c("z", 5_000, "p2wpkh"), c("a", 5_000, "p2wpkh"), c("m", 1_000, "p2wpkh")];
        // One tiny coin covers 500 plus its own ~141 sat fee.
        assert_eq!(ids(&select_smallest_first(&u, 500, 1.0).unwrap()), ["m"]);
        // Needs more than the tiny coin: next come the two equal coins, id order.
        assert_eq!(ids(&select_smallest_first(&u, 3_000, 1.0).unwrap()), ["m", "a"]);
    }

    #[test]
    fn fragmentation_is_none_when_all_the_coins_together_cannot_cover_amount_plus_fee() {
        let u = vec![c("a", 1_000, "p2wpkh"), c("b", 1_000, "p2wpkh")];
        assert_eq!(select_smallest_first(&u, 1_500, 10.0), None);
        assert_eq!(select_smallest_first(&[], 1, 1.0), None);
    }

    /// Over many pseudo-random wallets: a selection always pays amount plus its own fee, is a
    /// prefix of the smallest-first order, and None means even the whole wallet falls short.
    #[test]
    fn fragmentation_always_covers_amount_plus_fee_or_says_it_cannot() {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move |m: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        let scripts = ["p2wpkh", "p2tr", "p2sh", "p2pkh", "p2wsh"];
        for _ in 0..300 {
            let n = 1 + next(8) as usize;
            let u: Vec<Candidate> = (0..n).map(|i| c(&format!("c{i}"), 300 + next(60_000), scripts[next(5) as usize])).collect();
            let amount = 100 + next(120_000);
            let rate = 1.0 + next(60) as f64;
            let mut asc: Vec<&Candidate> = u.iter().collect();
            asc.sort_by(|a, b| a.amount_sats.cmp(&b.amount_sats).then_with(|| a.id.cmp(&b.id)));
            match select_smallest_first(&u, amount, rate) {
                Some(sel) => {
                    let picked: Vec<&Candidate> = sel.ids.iter().map(|id| u.iter().find(|c| &c.id == id).unwrap()).collect();
                    assert!(sel.total_sats >= amount + fee_for(&picked, rate), "short: {sel:?} amount {amount} rate {rate}");
                    let prefix: Vec<&str> = asc.iter().take(sel.ids.len()).map(|c| c.id.as_str()).collect();
                    assert_eq!(ids(&sel), prefix, "not the smallest-first prefix");
                }
                None => assert!(!covers(&asc, amount, rate), "said None but the whole wallet covers: {u:?} amount {amount} rate {rate}"),
            }
        }
    }
}
