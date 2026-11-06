//! Shape of a public fee estimate, filled by the optional explorer client
//! (`rpc::explorer`) only when the user has enabled it. This module makes no
//! network requests.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicFeeEstimate {
    pub fastest_fee: f64,
    pub half_hour_fee: f64,
    pub hour_fee: f64,
    pub economy_fee: f64,
    pub minimum_fee: f64,
}
