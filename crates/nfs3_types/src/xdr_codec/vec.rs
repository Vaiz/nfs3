use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::panic::RefUnwindSafe;
use std::{fmt, ptr, slice};

use super::{Decoder, Encoder, Error, Pack, Result, Unpack};

/// An XDR counted array with at most `N` elements.
///
/// Construct it with [`Self::from_array`] for a fixed-size array or [`TryFrom`]
/// for a vector. Use [`Default`] for an empty array.
///
/// Stores elements inline when the full bound fits in 64 bytes and the
/// element alignment is at most 8 bytes. Zero-sized elements and zero bounds
/// use `Vec`, which needs no allocation in those cases.
#[derive(Clone)]
pub struct BoundedVec<T, const N: usize>(Storage<T>);

#[derive(Clone)]
enum Storage<T> {
    Inline(Inline<T>),
    Heap(Vec<T>),
}

const INLINE_BYTES: usize = 64;

#[repr(C, align(8))]
struct InlineBytes {
    _bytes: [u8; INLINE_BYTES],
}

// Invariant: the first `len` slots in `data` contain owned, initialized
// values of T. T is nonzero-sized and its alignment fits InlineBytes.
struct Inline<T> {
    // T can have interior mutability even though the backing bytes do not.
    data: UnsafeCell<MaybeUninit<InlineBytes>>,
    len: usize,
    items: PhantomData<T>,
}

// SAFETY: shared access to the buffer only exposes a shared slice of T.
// Writing and dropping slots requires exclusive access.
unsafe impl<T: Sync> Sync for Inline<T> {}

impl<T: RefUnwindSafe> RefUnwindSafe for Inline<T> {}

impl<T> Inline<T> {
    const CAPACITY: usize = if align_of::<T>() <= align_of::<InlineBytes>() && size_of::<T>() != 0 {
        INLINE_BYTES / size_of::<T>()
    } else {
        0
    };

    const fn new() -> Self {
        assert!(Self::CAPACITY > 0);
        Self {
            data: UnsafeCell::new(MaybeUninit::uninit()),
            len: 0,
            items: PhantomData,
        }
    }

    fn from_items(items: impl IntoIterator<Item = T>) -> Self {
        let mut inline = Self::new();
        for item in items {
            inline.push(item);
        }
        inline
    }

    fn push(&mut self, item: T) {
        assert!(self.len < Self::CAPACITY);
        let data = self.data.get_mut().as_mut_ptr().cast::<T>();
        // SAFETY: `data` is aligned for T and `len` is an in-bounds,
        // uninitialized slot.
        unsafe { data.add(self.len).write(item) };
        self.len += 1;
    }

    const fn as_slice(&self) -> &[T] {
        let data = self.data.get().cast::<T>();
        // SAFETY: `data` is aligned for T and its first `len` slots
        // are initialized and borrowed for the lifetime of `self`.
        unsafe { slice::from_raw_parts(data, self.len) }
    }
}

impl<T: Clone> Clone for Inline<T> {
    fn clone(&self) -> Self {
        Self::from_items(self.as_slice().iter().cloned())
    }
}

impl<T> Drop for Inline<T> {
    fn drop(&mut self) {
        let data = self.data.get_mut().as_mut_ptr().cast::<T>();
        // SAFETY: the first `len` slots are initialized and exclusively
        // owned. Drop them once; the underlying byte buffer has no drop glue.
        unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(data, self.len)) };
    }
}

// Like Vec, BoundedVec does not expose a pinned projection to its elements.
impl<T, const N: usize> Unpin for BoundedVec<T, N> {}

impl<T, const N: usize> BoundedVec<T, N> {
    const INLINE: bool = N != 0 && N <= Inline::<T>::CAPACITY;

    /// Construct from an array whose length is checked at compile time.
    ///
    /// ```compile_fail
    /// use nfs3_types::xdr_codec::BoundedVec;
    ///
    /// let _ = BoundedVec::<u32, 1>::from_array([1, 2]);
    /// ```
    pub fn from_array<const LEN: usize>(items: [T; LEN]) -> Self {
        const { assert!(LEN <= N, "array exceeds the XDR element count limit") };
        if Self::INLINE {
            Self(Storage::Inline(Inline::from_items(items)))
        } else {
            Self(Storage::Heap(Vec::from(items)))
        }
    }
}

impl<T, const N: usize> Default for BoundedVec<T, N> {
    fn default() -> Self {
        if Self::INLINE {
            Self(Storage::Inline(Inline::new()))
        } else {
            Self(Storage::Heap(Vec::new()))
        }
    }
}

impl<T, const N: usize> TryFrom<Vec<T>> for BoundedVec<T, N> {
    type Error = Error;

    fn try_from(items: Vec<T>) -> Result<Self> {
        if items.len() > N {
            return Err(Error::InvalidLength(items.len()));
        }
        if Self::INLINE {
            Ok(Self(Storage::Inline(Inline::from_items(items))))
        } else {
            Ok(Self(Storage::Heap(items)))
        }
    }
}

impl<T, const N: usize> AsRef<[T]> for BoundedVec<T, N> {
    fn as_ref(&self) -> &[T] {
        match &self.0 {
            Storage::Inline(items) => items.as_slice(),
            Storage::Heap(items) => items,
        }
    }
}

impl<T, const N: usize> Deref for BoundedVec<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_ref()
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for BoundedVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BoundedVec").field(&self.as_ref()).finish()
    }
}

impl<T: PartialEq, const N: usize> PartialEq for BoundedVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: Eq, const N: usize> Eq for BoundedVec<T, N> {}

impl<T: Pack, const N: usize> Pack for BoundedVec<T, N> {
    fn packed_size(&self) -> usize {
        4 + self.iter().map(Pack::packed_size).sum::<usize>()
    }

    fn pack(&self, out: &mut Encoder) -> Result<()> {
        u32::try_from(self.len())
            .map_err(|_overflow| Error::ObjectTooLarge(self.len()))?
            .pack(out)?;
        for item in self.as_ref() {
            item.pack(out)?;
        }
        Ok(())
    }
}

impl<T: Unpack, const N: usize> Unpack for BoundedVec<T, N> {
    fn unpack(input: &mut Decoder) -> Result<Self> {
        let len = u32::unpack(input)? as usize;
        if len > N {
            return Err(Error::InvalidLength(len));
        }

        if Self::INLINE {
            let mut items = Inline::new();
            for _ in 0..len {
                items.push(T::unpack(input)?);
            }
            Ok(Self(Storage::Inline(items)))
        } else {
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(T::unpack(input)?);
            }
            Ok(Self(Storage::Heap(items)))
        }
    }
}
