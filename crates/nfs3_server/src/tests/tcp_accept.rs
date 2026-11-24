use std::io::{Error, ErrorKind};
use std::time::Duration;

use future_assert::FutureTestExt;
use tokio::time::Instant;

use crate::tcp::listener::accept_with_retry;

#[test]
fn accept_retries_without_delay() {
    let errors = [
        ErrorKind::ConnectionAborted.into(),
        ErrorKind::ConnectionReset.into(),
        ErrorKind::Interrupted.into(),
    ]
    .into_iter();
    #[cfg(target_os = "linux")]
    let errors = errors.chain(
        [
            libc::ENETDOWN,
            libc::EPROTO,
            libc::ENOPROTOOPT,
            libc::EHOSTDOWN,
            libc::ENONET,
            libc::EHOSTUNREACH,
            libc::EOPNOTSUPP,
            libc::ENETUNREACH,
        ]
        .map(Error::from_raw_os_error),
    );
    let mut results = errors.map(Err).chain([Ok(42)]);

    let accepted = accept_with_retry(|| std::future::ready(results.next().unwrap())).unwrap_ready();

    assert_eq!(accepted.unwrap(), 42);
    assert!(results.next().is_none());
}

#[tokio::test(start_paused = true)]
#[cfg(any(unix, windows))]
async fn accept_backs_off_under_resource_pressure() {
    #[cfg(unix)]
    let errors = [libc::EMFILE, libc::ENFILE, libc::ENOBUFS];
    #[cfg(windows)]
    let errors = {
        use windows_sys::Win32::Networking::WinSock::{WSAEMFILE, WSAENOBUFS};
        [WSAEMFILE, WSAENOBUFS]
    };
    let mut results = errors
        .into_iter()
        .cycle()
        .take(10)
        .map(Error::from_raw_os_error)
        .map(Err)
        .chain([Ok(())]);
    let mut attempts = Vec::new();
    accept_with_retry(|| {
        attempts.push(Instant::now());
        std::future::ready(results.next().unwrap())
    })
    .await
    .unwrap();

    for (index, pair) in attempts.windows(2).enumerate() {
        assert_eq!(
            pair[1] - pair[0],
            Duration::from_millis((10 << index.min(7)).min(1000))
        );
    }
}

#[test]
fn accept_stops_after_repeated_failures() {
    let mut attempts = 0;
    let err = accept_with_retry(|| {
        attempts += 1;
        std::future::ready(Err::<(), _>(ErrorKind::Interrupted.into()))
    })
    .unwrap_ready()
    .unwrap_err();
    assert_eq!(attempts, 16);
    assert_eq!(err.kind(), ErrorKind::Interrupted);
}

#[test]
fn accept_propagates_permanent_and_unknown_errors() {
    for error in [
        Error::from(ErrorKind::InvalidInput),
        Error::from(ErrorKind::OutOfMemory),
        Error::from_raw_os_error(i32::MAX),
    ] {
        let expected = (error.kind(), error.raw_os_error());
        let mut results = std::iter::once(Err::<(), _>(error));
        let err = accept_with_retry(|| std::future::ready(results.next().unwrap()))
            .unwrap_ready()
            .unwrap_err();
        assert_eq!((err.kind(), err.raw_os_error()), expected);
    }
}
