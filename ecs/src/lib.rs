#![no_std]

extern crate containers;
extern crate hash_table;
extern crate loca;
extern crate ptr as ptr_;
extern crate siphasher;
extern crate slot;

#[cfg(test)] extern crate default_allocator;

use containers::collections::RawVec;
use core::any::TypeId;
use core::{mem, ptr};
use core::ops::*;
use hash_table::HashTable;
use loca::*;
use ptr_::Unique;
use siphasher::sip;
use slot::Slot;

type Mask = u64;
type Version = u64;

const mask_size: usize = mem::size_of::<Mask>();
const component_n: usize = mask_size << 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Enty { mask: Mask, version: Version }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entity { index: usize, version: Version }

pub struct Components<A: Alloc> {
    entities: RawVec<Enty, A>,
    component_ptrs: HashTable<TypeId, *mut u8, CNArray<usize>, CNArray<Slot<(TypeId, *mut u8)>>,
                              sip::SipHasher>,
    droppers: [unsafe fn(*mut u8); component_n],
    layouts: [Layout; component_n],
}

impl<A: Alloc> Components<A> {
    #[inline]
    pub fn with_capacity_in(a: A, cap: usize) -> Option<Self> {
        let mut entities = RawVec::with_capacity_in(a, cap)?;
        for k in 0..cap { unsafe { entities.storage_mut()[k] = Enty { mask: 0, version: 0 }; } }
        let cps = HashTable::from_parts(CNArray([0; component_n]),
                                        unsafe { mem::uninitialized() },
                                        Default::default());
        Some(Components { entities, component_ptrs: cps,
                          droppers: unsafe { [mem::transmute(mem::align_of::<usize>()); component_n] },
                          layouts: unsafe { mem::uninitialized() } })
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
        let cap = self.entities.capacity();
        let alloc = unsafe { self.entities.alloc_mut() };
        let ptr: Unique<C> = if 0 == mem::size_of::<C>() { Unique::empty() }
                             else { alloc.alloc_array(cap).map_err(|_| Error(()))?.0 };
        let (k, _, _) = self.component_ptrs.insert_with(TypeId::of::<C>(), |p_opt| match p_opt {
            None => ptr.as_ptr() as _,
            Some(p) => unsafe {
                if 0 != mem::size_of::<C>() { let _ = alloc.dealloc_array(ptr, cap); }
                p
            },
        }).map_err(|(_, ptr)| { ptr(Some(0 as _)); Error(()) })?;
        self.droppers[k] = unsafe { mem::transmute::<unsafe fn(*mut C),
                                                     unsafe fn(*mut u8)>(ptr::drop_in_place::<C>) };
        self.layouts[k] = Layout::new::<C>();
        Ok(())
    }

    #[inline]
    fn component<C: 'static>(&self) -> Option<(usize, *mut C)> {
        self.component_ptrs.find_with_ix(&TypeId::of::<C>()).map(|(k, _, &ptr)| (k, ptr as _))
    }

    /// Get a reference to the component of type `C` of the given entity, if any.
    #[inline]
    pub fn get<C: 'static>(&self, Entity { index: k, version: v }: Entity) -> Option<&C> {
        let (ck, ptr) = self.component()?;
        unsafe {
            let e = self.entities.storage()[k];
            if v == e.version && 0 != e.mask & 1 << ck { ptr.offset(k as _).as_ref() }
            else { None }
        }
    }

    /// Modify the component of type `C` of the given entity, and whether it has such a
    /// component.
    #[inline]
    pub fn modify<C: 'static,
                  F: FnOnce(&mut Option<C>)>(&mut self,
                                             Entity { index: k, version: v }: Entity,
                                             f: F) { unsafe {
        let (ck, ptr) = match self.component() { None => return, Some(x) => x };
        let ptr = ptr.offset(k as _);
        let e = &mut self.entities.storage_mut()[k];
        if v != e.version { return; }
        let mut c_opt = if 0 == e.mask & 1 << ck { None } else {
            e.mask &= !(1 << ck);
            Some(ptr::read(ptr))
        };
        f(&mut c_opt);
        match c_opt {
            None => (),
            Some(x) => {
                e.mask |= 1 << ck;
                ptr::write(ptr, x);
            },
        }
    } }
}

#[derive(Clone, Copy, Debug)]
pub struct Error(());

impl<A: Alloc> Drop for Components<A> {
    fn drop(&mut self) {
        for (k, _, &ptr) in self.component_ptrs.iter_with_ix() { unsafe {
            let n = self.entities.capacity();
            let layout = self.layouts[k];
            let (array_layout, size) = layout.repeat(n).unwrap();

            struct Ptrs { ptr: *mut u8, end: *mut u8, size: usize }

            impl Iterator for Ptrs {
                type Item = *mut u8;
                fn next(&mut self) -> Option<*mut u8> {
                    if self.end == self.ptr { None }
                    else { let ptr = self.ptr; self.ptr = (ptr as usize + self.size) as _; Some(ptr) }
                }
            }

            for (&e, ptr) in Iterator::zip(self.entities.storage().iter(),
                                           Ptrs { ptr, end: ptr.wrapping_offset((size * n) as _), size }) {
                if 0 != e.mask & 1 << k { self.droppers[k](ptr); }
            }

            if 0 != layout.size() { self.entities.alloc_mut().dealloc(ptr, array_layout); }
        } }
    }
}

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

#[cfg(test)]
mod tests {
    #[test]
    fn test1() {
        let mut components = super::Components::with_capacity_in(::default_allocator::Heap::default(), 8).unwrap();
        components.reg::<()>().unwrap();
        components.reg::<u32>().unwrap();
    }
}
