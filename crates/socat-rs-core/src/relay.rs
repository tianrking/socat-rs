use crate::endpoint;
use crate::error::SocoreError;
use crate::spec::EndpointPlan;

pub async fn bridge_with_plans(left: EndpointPlan, right: EndpointPlan) -> Result<(), SocoreError> {
    let mut left = endpoint::open_with_options(left.endpoint, &left.options).await?;
    let mut right = endpoint::open_with_options(right.endpoint, &right.options).await?;
    tokio::io::copy_bidirectional(&mut left, &mut right).await?;
    Ok(())
}
