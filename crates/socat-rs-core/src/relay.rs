use serde::Serialize;

use crate::endpoint;
use crate::error::SocoreError;
use crate::spec::EndpointPlan;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RelayStats {
    pub bytes_left_to_right: u64,
    pub bytes_right_to_left: u64,
}

pub async fn bridge_with_plans(
    left: EndpointPlan,
    right: EndpointPlan,
) -> Result<RelayStats, SocoreError> {
    let mut left = endpoint::open_with_options(left.endpoint, &left.options).await?;
    let mut right = endpoint::open_with_options(right.endpoint, &right.options).await?;
    let (l2r, r2l) = tokio::io::copy_bidirectional(&mut left, &mut right).await?;
    Ok(RelayStats {
        bytes_left_to_right: l2r,
        bytes_right_to_left: r2l,
    })
}
