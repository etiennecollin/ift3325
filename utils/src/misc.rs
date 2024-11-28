//! Contains miscellaneous helper functions.

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task;

    #[tokio::test]
    async fn test_flatten_ok() {
        let handle = task::spawn(async { Ok::<_, &'static str>("Success") });
        let result = flatten(handle).await;
        assert_eq!(result, Ok("Success"));
    }

    #[tokio::test]
    async fn test_flatten_error() {
        let handle = task::spawn(async { Err::<(), _>("Failure") });
        let result = flatten(handle).await;
        assert_eq!(result, Err("Failure"));
    }

    #[tokio::test]
    async fn test_flatten_panicked() {
        let handle: tokio::task::JoinHandle<Result<(), &'static str>> = tokio::spawn(async {
            panic!("Oops");
        });

        let result = flatten(handle).await;

        assert!(result.is_err());
    }
}
