use std::time::Duration;

#[tokio::test]
async fn handshake_puzzle_math_and_bounds() {
    let mut x = [0; 64];
    x[63] = 4;
    let mut n = [0; 64];
    n[63] = 17;
    for (level, expected) in [(0, 4), (1, 16), (2, 1)] {
        let actual = tsproto::rsa_puzzle::solve(x, n, level).await.unwrap();
        assert_eq!(actual[63], expected);
        assert!(actual[..63].iter().all(|b| *b == 0));
    }
    assert!(tsproto::rsa_puzzle::solve(x, [0; 64], 1).await.is_err());
    assert!(tsproto::rsa_puzzle::solve(x, n, 100_001).await.is_err());
}

#[tokio::test]
async fn cancelled_handshakes_leave_async_timers_and_workers_available() {
    let mut x = [0; 64];
    x[63] = 7;
    let heavy = async {
        tokio::join!(
            tsproto::rsa_puzzle::solve(x, [255; 64], 100_000),
            tsproto::rsa_puzzle::solve(x, [255; 64], 100_000)
        )
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(1), heavy)
            .await
            .is_err()
    );
    let mut n = [0; 64];
    n[63] = 17;
    tokio::time::timeout(Duration::from_secs(1), tsproto::rsa_puzzle::solve(x, n, 1))
        .await
        .expect("cancelled CPU work must release its worker")
        .unwrap();
}
