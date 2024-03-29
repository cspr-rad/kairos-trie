use alloc::{boxed::Box, rc::Rc, string::String, sync::Arc, vec::Vec};

pub trait PortableHasher<const LEN: usize>: PortableUpdate + Default {
    fn finalize_reset(&mut self) -> [u8; LEN];
}

pub trait PortableUpdate {
    fn portable_update(&mut self, data: impl AsRef<[u8]>);
}

/// A wrapper around a `digest::Digest` that implements `PortableHasher`.
#[derive(Debug, Clone)]
pub struct DigestHasher<H: digest::Digest>(pub H);

impl<H: digest::Digest> Default for DigestHasher<H> {
    #[inline(always)]
    fn default() -> Self {
        Self(H::new())
    }
}

impl<const LEN: usize, H: digest::Digest + digest::FixedOutputReset> PortableHasher<LEN>
    for DigestHasher<H>
where
    digest::Output<H>: Into<[u8; LEN]>,
{
    #[inline(always)]
    fn finalize_reset(&mut self) -> [u8; LEN] {
        self.0.finalize_reset().into()
    }
}
impl<H: digest::Digest> PortableUpdate for DigestHasher<H> {
    #[inline(always)]
    fn portable_update(&mut self, data: impl AsRef<[u8]>) {
        self.0.update(data.as_ref());
    }
}

/// `std::portable_hash::portable_Hash` is not portable across platforms.
/// Implement this trait for a type that can be hashed in a portable way.
///
/// Note:
/// - types like uisize, and isize cannot be portably hashed.
/// - You must pick an endianness. Never use `to_ne_bytes`.
/// - Always use `to_le_bytes` or `to_be_bytes`.
///
/// All supported primitive types use `to_le_bytes`.
pub trait PortableHash {
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H);
}

impl PortableHash for () {
    #[inline(always)]
    fn portable_hash<H: PortableUpdate>(&self, _: &mut H) {}
}

impl PortableHash for u8 {
    #[inline(always)]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update([*self]);
    }
}
impl PortableHash for &u8 {
    #[inline(always)]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update([**self]);
    }
}

impl<const N: usize> PortableHash for [u8; N] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self);
    }
}
impl<const N: usize> PortableHash for &[u8; N] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(*self);
    }
}

impl PortableHash for [u8] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self);
    }
}
impl PortableHash for &[u8] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self);
    }
}

impl PortableHash for Vec<u8> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self);
    }
}
impl PortableHash for &Vec<u8> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self);
    }
}

impl PortableHash for bool {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update([*self as u8]);
    }
}
impl PortableHash for &bool {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update([**self as u8]);
    }
}
impl<const N: usize> PortableHash for [bool; N] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        (&self).portable_hash(hasher);
    }
}
impl<const N: usize> PortableHash for &[bool; N] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        let mut bytes = [0; N];
        for (i, item) in self.iter().enumerate() {
            bytes[i] = *item as u8;
        }
        hasher.portable_update(bytes);
    }
}
impl PortableHash for &[bool] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        for item in *self {
            item.portable_hash(hasher);
        }
    }
}
impl PortableHash for [bool] {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        (&self).portable_hash(hasher);
    }
}
impl PortableHash for Vec<bool> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        for item in self {
            item.portable_hash(hasher);
        }
    }
}
impl PortableHash for &Vec<bool> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        (*self).portable_hash(hasher);
    }
}

impl PortableHash for char {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update((*self as u32).to_le_bytes());
    }
}
impl PortableHash for &char {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update((**self as u32).to_le_bytes());
    }
}
impl PortableHash for Vec<char> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        for item in self {
            item.portable_hash(hasher);
        }
    }
}
impl PortableHash for &Vec<char> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        (*self).portable_hash(hasher);
    }
}

impl PortableHash for String {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self.as_bytes());
    }
}
impl PortableHash for &String {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self.as_bytes());
    }
}

macro_rules! impl_portable_hash {
    ($($t:ty),+) => {
        $(
            impl PortableHash for $t {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    hasher.portable_update(&self.to_le_bytes());
                }
            }

            impl<const N: usize> PortableHash for [$t; N] {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    for item in self {
                        item.portable_hash(hasher);
                    }
                }
            }

            impl<const N: usize> PortableHash for &[$t; N] {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    for item in *self {
                        item.portable_hash(hasher);
                    }
                }
            }

            impl PortableHash for [$t] {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    for item in self {
                        item.portable_hash(hasher);
                    }
                }
            }

            impl PortableHash for &[$t] {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    for item in *self {
                        item.portable_hash(hasher);
                    }
                }
            }

            impl PortableHash for Vec<$t> {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    for item in self {
                        item.portable_hash(hasher);
                    }
                }
            }

            impl PortableHash for &Vec<$t> {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    for item in *self {
                        item.portable_hash(hasher);
                    }
                }
            }
        )+
    };
}

impl_portable_hash!(u16, u32, u64, u128, i8, i16, i32, i64, i128);

macro_rules! impl_portable_hash_smart_ptr {
    ($($t:ty),+) => {
        $(
            impl<T: PortableHash> PortableHash for $t {
                #[inline]
                fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                    self.as_ref().portable_hash(hasher);
                }
            }
        )+
    };
}

impl_portable_hash_smart_ptr!(Box<T>, Rc<T>, Arc<T>);

macro_rules! impl_portable_hash_tuple {
    ($($t:ident),+) => {
        impl<$($t: PortableHash),+> PortableHash for ($($t,)+) {
            #[inline]
            fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
                #[allow(non_snake_case)]
                let ($($t,)+) = self;
                $($t.portable_hash(hasher);)+
            }
        }
    };
}

impl_portable_hash_tuple!(A, B);
impl_portable_hash_tuple!(A, B, C);
impl_portable_hash_tuple!(A, B, C, D);
impl_portable_hash_tuple!(A, B, C, D, E);
impl_portable_hash_tuple!(A, B, C, D, E, F);
impl_portable_hash_tuple!(A, B, C, D, E, F, G);
