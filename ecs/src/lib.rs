#![no_std]

extern crate containers;
extern crate hash_table;
extern crate loca;
extern crate ptr as ptr_;
extern crate siphasher;
extern crate slot;

use containers::collections::RawVec;
use core::any::TypeId;
use core::{mem, ptr, slice};
use core::ops::*;
use hash_table::HashTable;
use loca::*;
use ptr_::Unique;
use siphasher::sip;
use slot::Slot;

type Mask = u64;

const mask_size: usize = mem::size_of::<Mask>();
const component_n: usize = mask_size << 3;

pub struct Components<A: Alloc> {
    masks: RawVec<Mask, A>,
    component_ptrs: HashTable<TypeId, *mut u8, CNArray<usize>, CNArray<Slot<(TypeId, *mut u8)>>,
                              sip::SipHasher>,
    droppers: [fn(*mut u8, usize, &mut A); component_n],
}

impl<A: Alloc> Components<A> {
    #[inline]
    pub fn with_capacity_in(a: A, cap: usize) -> Option<Self> {
        let mut masks = RawVec::with_capacity_in(a, cap)?;
        for k in 0..cap { unsafe { masks.storage_mut()[k] = 0; } }
        let cps = HashTable::from_parts(CNArray([0; component_n]),
                                        unsafe { mem::uninitialized() },
                                        Default::default());
        fn no_drop<A>(_: *mut u8, _: usize, _: &mut A) {}
        Some(Components { masks, component_ptrs: cps, droppers: [no_drop; component_n] })
    }
}

impl<A: Alloc + Default> Components<A> {
    #[inline]
    pub fn with_capacity(cap: usize) -> Option<Self> {
        Self::with_capacity_in(A::default(), cap)
    }
}

impl<A: Alloc> Components<A> {
    /// Register the component type `C`.
    #[inline]
    pub fn reg<C: 'static>(&mut self) -> Result<(), Error> {
        let cap = self.masks.capacity();
        let alloc = unsafe { self.masks.alloc_mut() };
        let ptr: Unique<C> = alloc.alloc_array(cap).map_err(|_| Error(()))?.0;
        match self.component_ptrs.insert_with(TypeId::of::<C>(), |p_opt| match p_opt {
            None => ptr.as_ptr() as _,
            Some(p) => unsafe { let _ = alloc.dealloc_array(ptr, cap); p },
        }).map_err(|(_, ptr)| ptr(None)) {
            Ok((k, _, _)) => unsafe {
                ptr::write(self.droppers.as_mut_ptr().offset(k as _), drop_components::<C, A>);
                Ok(())
            },
            Err(ptr) => unsafe {
                let _ = alloc.dealloc_array(Unique::new_unchecked(ptr), cap);
                Err(Error(()))
            },
        }
    }

    #[inline]
    fn component<C: 'static>(&self) -> Option<(usize, *mut C)> {
        self.component_ptrs.find_with_ix(&TypeId::of::<C>()).map(|(k, _, &ptr)| (k, ptr as _))
    }

    /// Get a reference to the component of type `C` of the given entity `k`, if any.
    #[inline]
    pub fn get<C: 'static>(&self, k: usize) -> Option<&C> {
        let (ck, ptr) = self.component()?;
        unsafe { if 0 != self.masks.storage()[k] & 1 << ck { ptr.offset(k as _).as_ref() }
                 else { None } }
    }

    /// Modify the component of type `C` of the given entity `k`, and whether it has such a
    /// component.
    #[inline]
    pub fn modify<C: 'static, F: FnOnce(&mut Option<C>)>(&mut self, k: usize, f: F) { unsafe {
        let (ck, ptr) = match self.component() { None => return, Some(x) => x };
        let ptr = ptr.offset(k as _);
        let mut c_opt = if 0 == self.masks.storage()[k] & 1 << ck { None } else {
            self.masks.storage_mut()[k] &= !(1 << ck);
            Some(ptr::read(ptr))
        };
        f(&mut c_opt);
        match c_opt {
            None => (),
            Some(x) => {
                self.masks.storage_mut()[k] |= 1 << ck;
                ptr::write(ptr, x);
            },
        }
    } }
}

#[derive(Clone, Copy, Debug)]
pub struct Error(());

impl<A: Alloc> Drop for Components<A> {
    fn drop(&mut self) {
        for (k, _, &ptr) in self.component_ptrs.iter_with_ix() {
            self.droppers[k](ptr, self.masks.capacity(), unsafe { self.masks.alloc_mut() });
        }
    }
}

fn drop_components<C, A: Alloc>(ptr: *mut u8, n: usize, a: &mut A) { unsafe {
    let ptr = ptr as *mut C;
    for c in slice::from_raw_parts_mut(ptr, n) { ptr::drop_in_place(c); }
    let _ = a.dealloc_array(Unique::new_unchecked(ptr), n);
} }

#[derive(Clone, Copy)]
struct CNArray<A>([A; component_n]);

impl<A> Deref for CNArray<A> {
    type Target = [A];
    #[inline]
    fn deref(&self) -> &[A] { &self.0 }
}

impl<A> DerefMut for CNArray<A> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [A] { &mut self.0 }
}

impl<A> Index<usize> for CNArray<A> {
    type Output = A;
    #[inline]
    fn index(&self, k: usize) -> &A { &self.0[k] }
}

impl<A> IndexMut<usize> for CNArray<A> {
    #[inline]
    fn index_mut(&mut self, k: usize) -> &mut A { &mut self.0[k] }
}

impl<A> Index<::core::ops::RangeFull> for CNArray<A> {
    type Output = [A];
    #[inline]
    fn index(&self, _: ::core::ops::RangeFull) -> &[A] { &self.0[..] }
}

impl<A> IndexMut<::core::ops::RangeFull> for CNArray<A> {
    #[inline]
    fn index_mut(&mut self, _: ::core::ops::RangeFull) -> &mut [A] { &mut self.0[..] }
}
