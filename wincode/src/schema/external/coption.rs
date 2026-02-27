use {
    crate::{
        config::{self, ConfigCore},
        error::{invalid_tag_encoding, ReadResult, WriteResult},
        io::{Reader, Writer},
        schema::{SchemaRead, SchemaWrite, TypeMeta},
    },
    core::mem::MaybeUninit,
    solana_program_option::COption,
};

unsafe impl<'de, T, C: ConfigCore> SchemaRead<'de, C> for COption<T>
where
    T: SchemaRead<'de, C>,
{
    type Dst = COption<T::Dst>;

    #[inline]
    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let variant = <u32 as SchemaRead<'de, C>>::get(reader.by_ref())?;
        match variant {
            0 => dst.write(COption::None),
            1 => dst.write(COption::Some(T::get(reader)?)),
            _ => return Err(invalid_tag_encoding(variant as usize)),
        };
        Ok(())
    }
}

unsafe impl<T, C: ConfigCore> SchemaWrite<C> for COption<T>
where
    T: SchemaWrite<C>,
    T::Src: Sized,
{
    type Src = COption<T::Src>;

    #[inline]
    #[allow(clippy::arithmetic_side_effects)]
    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        match src {
            COption::Some(value) => Ok(4 + T::size_of(value)?),
            COption::None => Ok(4),
        }
    }

    #[inline]
    fn write(mut writer: impl Writer, value: &Self::Src) -> WriteResult<()> {
        match value {
            COption::Some(inner) => {
                <u32 as SchemaWrite<C>>::write(writer.by_ref(), &1u32)?;
                T::write(writer, inner)
            }
            COption::None => <u32 as SchemaWrite<C>>::write(writer, &0u32),
        }
    }
}

/// Mutable view into a serialized [`COption<T>`].
///
/// Both the discriminant and value are mutable references into the
/// underlying buffer, enabling in-place mutation of account state
/// without reallocation.
///
/// Produced by deserializing with [`wincode::deserialize_mut`](crate::deserialize_mut)
/// on a struct field of type `COptionMut<'a, T>`.
#[repr(C)]
#[derive(Debug)]
pub struct COptionMut<'a, T> {
    disc: &'a mut [u8; 4],
    value: &'a mut T,
}

impl<T> COptionMut<'_, T> {
    /// Returns `true` if the discriminant indicates `Some`.
    pub fn is_some(&self) -> bool {
        u32::from_le_bytes(*self.disc) == 1
    }

    /// Returns `true` if the discriminant indicates `None`.
    pub fn is_none(&self) -> bool {
        u32::from_le_bytes(*self.disc) == 0
    }

    /// Returns `true` if the option is `Some` and the value matches the given value.
    pub fn contains<U>(&self, x: &U) -> bool
    where
        T: PartialEq<U>,
    {
        match self.as_ref() {
            Some(v) => v == x,
            None => false,
        }
    }

    /// Returns a reference to the inner value if the discriminant indicates `Some`.
    pub fn as_ref(&self) -> Option<&T> {
        if self.is_some() {
            Some(self.value)
        } else {
            None
        }
    }

    /// Returns a mutable reference to the inner value if the discriminant indicates `Some`.
    pub fn as_mut(&mut self) -> Option<&mut T> {
        if self.is_some() {
            Some(self.value)
        } else {
            None
        }
    }

    /// Returns a reference to the contained value, panicking with the given message if `None`.
    pub fn expect(&self, msg: &str) -> &T {
        match self.as_ref() {
            Some(v) => v,
            None => panic!("{}", msg),
        }
    }

    /// Returns a reference to the contained value, panicking if `None`.
    pub fn unwrap(&self) -> &T {
        self.expect("called `COptionMut::unwrap()` on a `None` value")
    }

    /// Set the option to `Some(v)`, writing both the discriminant and value.
    pub fn set_some(&mut self, v: T) {
        *self.disc = 1u32.to_le_bytes();
        *self.value = v;
    }

    /// Set the option to `None`, writing just the discriminant.
    pub fn set_none(&mut self) {
        *self.disc = 0u32.to_le_bytes();
    }

    /// Inserts `v` if the option is `None`, then returns a mutable reference to the value.
    pub fn get_or_insert(&mut self, v: T) -> &mut T {
        if self.is_none() {
            self.set_some(v);
        }
        self.value
    }

    /// Inserts a value computed by `f` if the option is `None`,
    /// then returns a mutable reference to the value.
    pub fn get_or_insert_with<F: FnOnce() -> T>(&mut self, f: F) -> &mut T {
        if self.is_none() {
            self.set_some(f());
        }
        self.value
    }
}

unsafe impl<'de, T, C: ConfigCore> SchemaRead<'de, C> for COptionMut<'de, T>
where
    T: SchemaRead<'de, C> + config::ZeroCopy<C>,
    T::Dst: 'static,
{
    type Dst = COptionMut<'de, T::Dst>;

    const TYPE_META: TypeMeta = match T::TYPE_META {
        TypeMeta::Static { size, .. } => TypeMeta::Static {
            size: 4 + size,
            zero_copy: false,
        },
        TypeMeta::Dynamic => TypeMeta::Dynamic,
    };

    #[inline]
    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        // Read discriminant as &mut [u8; 4] (always zero-copy).
        let disc = <&'de mut [u8; 4] as SchemaRead<'de, C>>::get(reader.by_ref())?;
        // Always read value as &mut T (zero-copy into buffer), regardless of disc.
        let value = <&'de mut T as SchemaRead<'de, C>>::get(reader)?;
        dst.write(COptionMut { disc, value });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{deserialize, serialize},
    };

    #[test]
    fn test_coption_roundtrip_some() {
        let value: COption<u32> = COption::Some(42);
        let serialized = serialize(&value).unwrap();
        // u32 disc (1) + u32 payload (42)
        assert_eq!(serialized, [1, 0, 0, 0, 42, 0, 0, 0]);
        let deserialized: COption<u32> = deserialize(&serialized).unwrap();
        assert_eq!(deserialized, COption::Some(42));
    }

    #[test]
    fn test_coption_roundtrip_none() {
        let value: COption<u32> = COption::None;
        let serialized = serialize(&value).unwrap();
        // u32 disc (0), no payload
        assert_eq!(serialized, [0, 0, 0, 0]);
        let deserialized: COption<u32> = deserialize(&serialized).unwrap();
        assert_eq!(deserialized, COption::None);
    }

    #[test]
    fn test_coption_invalid_discriminant() {
        let bad = [2u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
        let result = deserialize::<COption<u32>>(&bad);
        assert!(result.is_err());
    }

    #[test]
    fn test_coption_mut_read_some() {
        // Some(0xAABBCCDD) as le bytes: disc=1, payload=[0xDD, 0xCC, 0xBB, 0xAA]
        let mut buf = [1, 0, 0, 0, 0xDD, 0xCC, 0xBB, 0xAA];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert!(opt.is_some());
        assert_eq!(*opt.as_ref().unwrap(), [0xDD, 0xCC, 0xBB, 0xAA]);
    }

    #[test]
    fn test_coption_mut_read_none() {
        let mut buf = [0, 0, 0, 0, 0, 0, 0, 0];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert!(opt.is_none());
    }

    #[test]
    fn test_coption_mut_set_some() {
        let mut buf = [0u8; 8];
        let mut opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert!(opt.is_none());

        opt.set_some([0x11, 0x22, 0x33, 0x44]);
        assert!(opt.is_some());
        // Verify mutation is reflected in the underlying buffer.
        assert_eq!(buf, [1, 0, 0, 0, 0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn test_coption_mut_set_none() {
        let mut buf = [1, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert!(opt.is_some());

        opt.set_none();
        assert!(opt.is_none());
        // Discriminant zeroed, payload untouched.
        assert_eq!(buf, [0, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_coption_mut_contains() {
        let mut buf = [1, 0, 0, 0, 0x42, 0, 0, 0];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert!(opt.contains(&[0x42, 0, 0, 0]));
        assert!(!opt.contains(&[0xFF, 0, 0, 0]));
    }

    #[test]
    fn test_coption_mut_contains_none() {
        let mut buf = [0u8; 8];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert!(!opt.contains(&[0, 0, 0, 0]));
    }

    #[test]
    fn test_coption_mut_unwrap() {
        let mut buf = [1, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert_eq!(*opt.unwrap(), [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    #[should_panic(expected = "None")]
    fn test_coption_mut_unwrap_none_panics() {
        let mut buf = [0u8; 8];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        opt.unwrap();
    }

    #[test]
    fn test_coption_mut_as_mut_unwrap() {
        let mut buf = [1, 0, 0, 0, 0, 0, 0, 0];
        let mut opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        *opt.as_mut().unwrap() = [0x11, 0x22, 0x33, 0x44];
        assert_eq!(buf, [1, 0, 0, 0, 0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn test_coption_mut_expect() {
        let mut buf = [1, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        assert_eq!(*opt.expect("should be Some"), [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    #[should_panic(expected = "custom msg")]
    fn test_coption_mut_expect_none_panics() {
        let mut buf = [0u8; 8];
        let opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        opt.expect("custom msg");
    }

    #[test]
    fn test_coption_mut_get_or_insert() {
        let mut buf = [0u8; 8];
        let mut opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        let val = opt.get_or_insert([0x11, 0x22, 0x33, 0x44]);
        assert_eq!(*val, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(buf, [1, 0, 0, 0, 0x11, 0x22, 0x33, 0x44]);
    }

    #[test]
    fn test_coption_mut_get_or_insert_existing() {
        let mut buf = [1, 0, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        let val = opt.get_or_insert([0x11, 0x22, 0x33, 0x44]);
        // Existing value preserved.
        assert_eq!(*val, [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_coption_mut_get_or_insert_with() {
        let mut buf = [0u8; 8];
        let mut opt: COptionMut<'_, [u8; 4]> = crate::deserialize_mut(&mut buf).unwrap();
        let val = opt.get_or_insert_with(|| [0xFF; 4]);
        assert_eq!(*val, [0xFF; 4]);
        assert_eq!(buf, [1, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_coption_serialized_size() {
        let some: COption<[u8; 32]> = COption::Some([0xFF; 32]);
        let none: COption<[u8; 32]> = COption::None;
        assert_eq!(crate::serialized_size(&some).unwrap(), 4 + 32);
        assert_eq!(crate::serialized_size(&none).unwrap(), 4);
    }
}
