use super::*;
use crate::xdr_codec::BackingBudget;

struct Input(std::io::Cursor<Vec<u8>>);
impl AsyncRead for Input {
    async fn async_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, buf)
    }
}

// Public used() includes unused reservations, so physical allocation needs a
// private counter. All wire/IO/semantic tests live in the crate tests directory.
#[test]
fn no_body_header_one_byte_and_terminal_allocation_bounds() {
    let mut future = std::pin::pin!(async {
        let mut policy = RecordLimits {
            block_bytes: 64,
            max_record_bytes: 1024,
            max_fragments: 128,
        };
        assert!(record_reservation_bound(usize::MAX).is_none());
        for invalid in [
            RecordLimits {
                block_bytes: 0,
                ..policy
            },
            RecordLimits {
                max_record_bytes: 0,
                ..policy
            },
            RecordLimits {
                max_record_bytes: 0x8000_0000,
                ..policy
            },
            RecordLimits {
                max_fragments: 0,
                ..policy
            },
        ] {
            let budget = BackingBudget::new(1024);
            let mut reservation = budget.try_reserve(1024).unwrap();
            let mut input = Input(std::io::Cursor::new(vec![0x80, 0, 0, 0]));
            assert!(matches!(
                receive(&mut input, invalid, &mut reservation).await,
                Err(FramingError::Limit)
            ));
            assert_eq!(
                input.0.position(),
                0,
                "invalid policy must not consume the marker"
            );
            assert_eq!(reservation.remaining(), 1024);
            drop(reservation);
            assert_eq!(budget.used(), 0);
        }
        for (marker, body, expected) in [
            (0x8000_0000_u32, vec![], (0, 0)),
            (1024, vec![1], (1, 64)),
            (0x8000_0001, vec![1], (1, 1)),
        ] {
            let mut wire = marker.to_be_bytes().to_vec();
            wire.extend(body);
            let budget = BackingBudget::new(1024);
            let mut reservation = budget.try_reserve(1024).unwrap();
            RECEIVE_ALLOCATIONS.set((0, 0));
            drop(
                receive(
                    &mut Input(std::io::Cursor::new(wire)),
                    policy,
                    &mut reservation,
                )
                .await,
            );
            assert_eq!(RECEIVE_ALLOCATIONS.get(), expected);
            drop(reservation);
            assert_eq!(budget.used(), 0);
        }
        policy.max_record_bytes = 31;
        let budget = BackingBudget::new(31);
        let mut reservation = budget.try_reserve(31).unwrap();
        RECEIVE_ALLOCATIONS.set((0, 0));
        drop(
            receive(
                &mut Input(std::io::Cursor::new(vec![0, 0, 0, 31, 1])),
                policy,
                &mut reservation,
            )
            .await,
        );
        assert_eq!(RECEIVE_ALLOCATIONS.get(), (1, 31));
    });
    assert!(matches!(
        future
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop())),
        std::task::Poll::Ready(())
    ));
}
