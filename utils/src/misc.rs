use tokio::task::JoinHandle;

/// Flattens the result of a join handle.
/// The function awaits the result of the join handle and returns the inner result or error.
pub async fn flatten<T>(handle: JoinHandle<Result<T, &'static str>>) -> Result<T, &'static str> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(_) => Err("Handling failed"),
    }
}
