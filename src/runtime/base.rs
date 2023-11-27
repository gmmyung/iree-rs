use std::{ffi::c_void, fmt::Display};

use iree_sys::runtime as sys;
use log::trace;

pub struct ByteSpan<'a> {
    pub(crate) ctx: sys::iree_byte_span_t,
    marker: std::marker::PhantomData<&'a mut [u8]>,
}

impl<'a> From<&'a mut [u8]> for ByteSpan<'a> {
    fn from(slice: &'a mut [u8]) -> Self {
        let byte_span = sys::iree_byte_span_t {
            data: slice.as_ptr() as *mut u8,
            data_length: slice.len(),
        };
        Self {
            ctx: byte_span,
            marker: std::marker::PhantomData,
        }
    }
}

impl<'a> From<ByteSpan<'a>> for &'a mut [u8] {
    fn from(byte_span: ByteSpan<'a>) -> Self {
        unsafe { std::slice::from_raw_parts_mut(byte_span.ctx.data, byte_span.ctx.data_length) }
    }
}

pub struct ConstByteSpan<'a> {
    pub ctx: sys::iree_const_byte_span_t,
    marker: std::marker::PhantomData<&'a [u8]>,
}

impl<'a> From<&'a [u8]> for ConstByteSpan<'a> {
    fn from(slice: &'a [u8]) -> Self {
        let byte_span = sys::iree_const_byte_span_t {
            data: slice.as_ptr() as *const u8,
            data_length: slice.len(),
        };
        Self {
            ctx: byte_span,
            marker: std::marker::PhantomData,
        }
    }
}

impl<'a> From<ConstByteSpan<'a>> for &'a [u8] {
    fn from(byte_span: ConstByteSpan<'a>) -> Self {
        unsafe { std::slice::from_raw_parts(byte_span.ctx.data, byte_span.ctx.data_length) }
    }
}

pub struct StringView<'a> {
    pub ctx: sys::iree_string_view_t,
    marker: std::marker::PhantomData<&'a mut str>,
}

impl Display for StringView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.ctx.data as *const u8,
                self.ctx.size,
            ))
        })
    }
}

impl<'a> From<&'a str> for StringView<'a> {
    fn from(s: &'a str) -> Self {
        let string_view = sys::iree_string_view_t {
            data: s.as_ptr() as *mut i8,
            size: s.len(),
        };
        Self {
            ctx: string_view,
            marker: std::marker::PhantomData,
        }
    }
}

impl<'a> From<StringView<'a>> for &'a str {
    fn from(string_view: StringView<'a>) -> Self {
        unsafe {
            std::str::from_utf8_unchecked_mut(std::slice::from_raw_parts_mut(
                string_view.ctx.data as *mut u8,
                string_view.ctx.size,
            ))
        }
    }
}

pub(crate) struct Allocator {
    pub(crate) ctx: sys::iree_allocator_t,
}

impl Allocator {
    pub fn get_global() -> Self {
        let allocator = sys::iree_allocator_t {
            self_: std::ptr::null_mut(),
            ctl: Some(rust_allocator_ctl),
        };
        Self { ctx: allocator }
    }

    pub fn null_allocator() -> Self {
        let allocator = sys::iree_allocator_t {
            self_: std::ptr::null_mut(),
            ctl: Some(null_allocator_ctl),
        };
        Self { ctx: allocator }
    }
}

const ALIGNMENT: usize = 16;

unsafe extern "C" fn null_allocator_ctl(
    _self_: *mut c_void,
    command: sys::iree_allocator_command_e,
    _params: *const c_void,
    inout_ptr: *mut *mut c_void,
) -> sys::iree_status_t {
    match command {
        sys::iree_allocator_command_e_IREE_ALLOCATOR_COMMAND_FREE => {
            trace!(
                "null_allocator_ctl: IREE_ALLOCATOR_COMMAND_FREE, {:p}",
                *inout_ptr
            );
        }
        _ => {
            trace!("null_allocator_ctl: command: {:?}", command);
        }
    }
    std::ptr::null_mut() as *mut c_void as sys::iree_status_t
}

unsafe extern "C" fn rust_allocator_ctl(
    _self_: *mut c_void,
    command: sys::iree_allocator_command_e,
    params: *const c_void,
    inout_ptr: *mut *mut c_void,
) -> sys::iree_status_t {
    // use Rust Global Allocator
    match command {
        sys::iree_allocator_command_e_IREE_ALLOCATOR_COMMAND_MALLOC => {
            let size = (*(params as *const sys::iree_allocator_alloc_params_t)).byte_length;
            if size > std::isize::MAX as usize {
                return Status::from_code(StatusErrorKind::OutOfRange).ctx;
            }
            let ptr = std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(
                size + ALIGNMENT,
                ALIGNMENT,
            ));
            *(ptr as *mut usize) = size;
            *inout_ptr = ptr.wrapping_add(ALIGNMENT) as *mut c_void;
            trace!(
                "rust_allocator_ctl: IREE_ALLOCATOR_COMMAND_MALLOC: size: {} -> {:?}",
                size, *inout_ptr
            );
            std::ptr::null_mut() as *mut c_void as sys::iree_status_t
        }
        sys::iree_allocator_command_e_IREE_ALLOCATOR_COMMAND_CALLOC => {
            let size = (*(params as *const sys::iree_allocator_alloc_params_t)).byte_length;
            if size > std::isize::MAX as usize {
                return Status::from_code(StatusErrorKind::OutOfRange).ctx;
            }
            let ptr = std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align_unchecked(
                size + ALIGNMENT,
                ALIGNMENT,
            ));
            *(ptr as *mut usize) = size;
            *inout_ptr = ptr.wrapping_add(ALIGNMENT) as *mut c_void;
            trace!(
                "rust_allocator_ctl: IREE_ALLOCATOR_COMMAND_CALLOC: size: {} -> {:?}",
                size, *inout_ptr
            );
            std::ptr::null_mut() as *mut c_void as sys::iree_status_t
        }
        sys::iree_allocator_command_e_IREE_ALLOCATOR_COMMAND_REALLOC => {
            if *inout_ptr == std::ptr::null_mut() {
                // realloc of null is malloc
                return rust_allocator_ctl(
                    _self_,
                    sys::iree_allocator_command_e_IREE_ALLOCATOR_COMMAND_MALLOC,
                    params,
                    inout_ptr,
                );
            }
            let ptr = (*inout_ptr).wrapping_sub(ALIGNMENT);
            let old_size = unsafe { *(ptr as *mut usize) };
            let new_size = (*(params as *const sys::iree_allocator_alloc_params_t)).byte_length;
            trace!(
                "rust_allocator_ctl: IREE_ALLOCATOR_COMMAND_REALLOC: {} -> {}",
                old_size, new_size
            );
            if new_size > std::isize::MAX as usize {
                return Status::from_code(StatusErrorKind::OutOfRange).ctx;
            }
            let ptr = std::alloc::realloc(
                ptr as *mut u8,
                std::alloc::Layout::from_size_align_unchecked(old_size + ALIGNMENT, ALIGNMENT),
                new_size + ALIGNMENT,
            );
            unsafe {
                *(ptr as *mut usize) = new_size;
            }
            *inout_ptr = ptr.wrapping_add(ALIGNMENT) as *mut c_void;
            std::ptr::null_mut() as *mut c_void as sys::iree_status_t
        }
        sys::iree_allocator_command_e_IREE_ALLOCATOR_COMMAND_FREE => {
            let ptr = (*inout_ptr).wrapping_sub(ALIGNMENT);
            let size = unsafe { *(ptr as *mut usize) };
            trace!(
                "rust_allocator_ctl: IREE_ALLOCATOR_COMMAND_FREE: size: {}->{:p}",
                size, *inout_ptr
            );
            std::alloc::dealloc(
                ptr as *mut u8,
                std::alloc::Layout::from_size_align_unchecked(size + ALIGNMENT, ALIGNMENT),
            );
            std::ptr::null_mut() as *mut c_void as sys::iree_status_t
        }
        _ => Status::from_code(StatusErrorKind::Unimplemented).ctx,
    }
}

pub struct Status {
    ctx: sys::iree_status_t,
}

impl Status {
    pub(crate) fn from_raw(ctx: sys::iree_status_t) -> Self {
        Self { ctx }
    }

    pub(crate) fn from_code(status_kind: StatusErrorKind) -> Self {
        let status: sys::iree_status_code_e = status_kind.into();
        Status {
            ctx: &STATUS_CODES[status as usize] as *const usize as *mut usize as *mut _,
        }
    }

    pub(crate) fn is_ok(&self) -> bool {
        self.ctx as usize == 0
    }

    pub fn to_result(self) -> Result<(), StatusError> {
        if self.is_ok() {
            Ok(())
        } else {
            Err(StatusError { status: self })
        }
    }

    pub fn chain(self, other: Self) -> Self {
        Self {
            ctx: unsafe { sys::iree_status_join(self.ctx, other.ctx) },
        }
    }
}

impl std::fmt::Debug for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

impl Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut bufptr = std::ptr::null_mut();
        let allocator = Allocator::get_global();
        let mut size: usize = 0;
        if !(unsafe {
            sys::iree_status_to_string(self.status.ctx, &allocator.ctx, &mut bufptr, &mut size)
        }) {
            return write!(f, "Status: <failed to convert to string>");
        }
        let buf =
            std::str::from_utf8(unsafe { std::slice::from_raw_parts(bufptr as *const u8, size) })
                .map_err(|_| std::fmt::Error)?;
        let write_result = write!(f, "Status: {:?}", buf);
        unsafe {
            sys::iree_allocator_free(allocator.ctx, bufptr as *mut _);
        }
        write_result
    }
}

pub struct StatusError {
    status: Status,
}

impl std::error::Error for StatusError {}

impl<'a, 'b> Drop for Status {
    fn drop(&mut self) {
        unsafe {
            if !self.is_ok() {
                sys::iree_status_ignore(self.ctx);
            }
        }
    }
}

// Necessary because status code lifetime is not specified in the C API
static STATUS_CODES: [usize; 18] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];

pub enum StatusErrorKind {
    Cancelled,
    Unknown,
    InvalidArgument,
    DeadlineExceeded,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    ResourceExhausted,
    FailedPrecondition,
    Aborted,
    OutOfRange,
    Unimplemented,
    Internal,
    Unavailable,
    DataLoss,
    Unauthenticated,
    Deferred,
    UnknownStatus,
}

impl From<sys::iree_status_code_e> for StatusErrorKind {
    fn from(status: sys::iree_status_code_e) -> Self {
        match status {
            sys::iree_status_code_e_IREE_STATUS_CANCELLED => Self::Cancelled,
            sys::iree_status_code_e_IREE_STATUS_UNKNOWN => Self::Unknown,
            sys::iree_status_code_e_IREE_STATUS_INVALID_ARGUMENT => Self::InvalidArgument,
            sys::iree_status_code_e_IREE_STATUS_DEADLINE_EXCEEDED => Self::DeadlineExceeded,
            sys::iree_status_code_e_IREE_STATUS_NOT_FOUND => Self::NotFound,
            sys::iree_status_code_e_IREE_STATUS_ALREADY_EXISTS => Self::AlreadyExists,
            sys::iree_status_code_e_IREE_STATUS_PERMISSION_DENIED => Self::PermissionDenied,
            sys::iree_status_code_e_IREE_STATUS_RESOURCE_EXHAUSTED => Self::ResourceExhausted,
            sys::iree_status_code_e_IREE_STATUS_FAILED_PRECONDITION => Self::FailedPrecondition,
            sys::iree_status_code_e_IREE_STATUS_ABORTED => Self::Aborted,
            sys::iree_status_code_e_IREE_STATUS_OUT_OF_RANGE => Self::OutOfRange,
            sys::iree_status_code_e_IREE_STATUS_UNIMPLEMENTED => Self::Unimplemented,
            sys::iree_status_code_e_IREE_STATUS_INTERNAL => Self::Internal,
            sys::iree_status_code_e_IREE_STATUS_UNAVAILABLE => Self::Unavailable,
            sys::iree_status_code_e_IREE_STATUS_DATA_LOSS => Self::DataLoss,
            sys::iree_status_code_e_IREE_STATUS_UNAUTHENTICATED => Self::Unauthenticated,
            sys::iree_status_code_e_IREE_STATUS_DEFERRED => Self::Deferred,
            _ => Self::UnknownStatus,
        }
    }
}

impl From<StatusErrorKind> for sys::iree_status_code_t {
    fn from(status: StatusErrorKind) -> Self {
        use StatusErrorKind::*;
        match status {
            Cancelled => sys::iree_status_code_e_IREE_STATUS_CANCELLED,
            Unknown => sys::iree_status_code_e_IREE_STATUS_UNKNOWN,
            InvalidArgument => sys::iree_status_code_e_IREE_STATUS_INVALID_ARGUMENT,
            DeadlineExceeded => sys::iree_status_code_e_IREE_STATUS_DEADLINE_EXCEEDED,
            NotFound => sys::iree_status_code_e_IREE_STATUS_NOT_FOUND,
            AlreadyExists => sys::iree_status_code_e_IREE_STATUS_ALREADY_EXISTS,
            PermissionDenied => sys::iree_status_code_e_IREE_STATUS_PERMISSION_DENIED,
            ResourceExhausted => sys::iree_status_code_e_IREE_STATUS_RESOURCE_EXHAUSTED,
            FailedPrecondition => sys::iree_status_code_e_IREE_STATUS_FAILED_PRECONDITION,
            Aborted => sys::iree_status_code_e_IREE_STATUS_ABORTED,
            OutOfRange => sys::iree_status_code_e_IREE_STATUS_OUT_OF_RANGE,
            Unimplemented => sys::iree_status_code_e_IREE_STATUS_UNIMPLEMENTED,
            Internal => sys::iree_status_code_e_IREE_STATUS_INTERNAL,
            Unavailable => sys::iree_status_code_e_IREE_STATUS_UNAVAILABLE,
            DataLoss => sys::iree_status_code_e_IREE_STATUS_DATA_LOSS,
            Unauthenticated => sys::iree_status_code_e_IREE_STATUS_UNAUTHENTICATED,
            Deferred => sys::iree_status_code_e_IREE_STATUS_DEFERRED,
            UnknownStatus => panic!("Unknown status"),
        }
    }
}
