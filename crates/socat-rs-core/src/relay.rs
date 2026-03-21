use crate::endpoint;
use crate::error::SocoreError;
use crate::spec::EndpointSpec;

pub async fn bridge(left: EndpointSpec, right: EndpointSpec) -> Result<(), SocoreError> {
    let mut left = endpoint::open(left).await?;
    let mut right = endpoint::open(right).await?;
    tokio::io::copy_bidirectional(&mut left, &mut right).await?;
    Ok(())
}
