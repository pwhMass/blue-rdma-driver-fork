use std::net::Ipv4Addr;

use crate::verbs::dev::PciHwDevice;

use super::{
    ctx::{HwDeviceCtx, VerbsOps},
    dev::EmulatedHwDevice,
    mock::MockDeviceCtx,
};

/// RDMA context operations for Blue-RDMA driver.
///
/// # Safety
/// Implementors must ensure all FFI and RDMA verbs specification requirements are met,
pub unsafe trait RdmaCtxOps {
    fn init();

    #[allow(clippy::new_ret_no_self)]
    /// Safety: caller must ensure `sysfs_name` is a valid pointer
    fn new(sysfs_name: *const std::ffi::c_char) -> *mut std::ffi::c_void;

    fn free(driver_data: *const std::ffi::c_void);

    fn alloc_pd(blue_context: *mut ibverbs_sys::ibv_context) -> *mut ibverbs_sys::ibv_pd;

    fn dealloc_pd(pd: *mut ibverbs_sys::ibv_pd) -> ::std::os::raw::c_int;

    fn query_device_ex(
        blue_context: *mut ibverbs_sys::ibv_context,
        _input: *const ibverbs_sys::ibv_query_device_ex_input,
        device_attr: *mut ibverbs_sys::ibv_device_attr,
        _attr_size: usize,
    ) -> ::std::os::raw::c_int;

    fn query_port(
        blue_context: *mut ibverbs_sys::ibv_context,
        port_num: u8,
        port_attr: *mut ibverbs_sys::ibv_port_attr,
    ) -> ::std::os::raw::c_int;

    fn create_cq(
        blue_context: *mut ibverbs_sys::ibv_context,
        cqe: core::ffi::c_int,
        channel: *mut ibverbs_sys::ibv_comp_channel,
        comp_vector: core::ffi::c_int,
    ) -> *mut ibverbs_sys::ibv_cq;

    fn destroy_cq(cq: *mut ibverbs_sys::ibv_cq) -> ::std::os::raw::c_int;

    fn create_qp(
        pd: *mut ibverbs_sys::ibv_pd,
        init_attr: *mut ibverbs_sys::ibv_qp_init_attr,
    ) -> *mut ibverbs_sys::ibv_qp;

    fn destroy_qp(qp: *mut ibverbs_sys::ibv_qp) -> ::std::os::raw::c_int;

    fn modify_qp(
        qp: *mut ibverbs_sys::ibv_qp,
        attr: *mut ibverbs_sys::ibv_qp_attr,
        attr_mask: core::ffi::c_int,
    ) -> ::std::os::raw::c_int;

    fn query_qp(
        qp: *mut ibverbs_sys::ibv_qp,
        attr: *mut ibverbs_sys::ibv_qp_attr,
        attr_mask: core::ffi::c_int,
        init_attr: *mut ibverbs_sys::ibv_qp_init_attr,
    ) -> ::std::os::raw::c_int;

    fn reg_mr(
        pd: *mut ibverbs_sys::ibv_pd,
        addr: *mut ::std::os::raw::c_void,
        length: usize,
        _hca_va: u64,
        access: core::ffi::c_int,
    ) -> *mut ibverbs_sys::ibv_mr;

    fn dereg_mr(mr: *mut ibverbs_sys::ibv_mr) -> ::std::os::raw::c_int;

    fn post_send(
        qp: *mut ibverbs_sys::ibv_qp,
        wr: *mut ibverbs_sys::ibv_send_wr,
        bad_wr: *mut *mut ibverbs_sys::ibv_send_wr,
    ) -> ::std::os::raw::c_int;

    fn post_recv(
        qp: *mut ibverbs_sys::ibv_qp,
        wr: *mut ibverbs_sys::ibv_recv_wr,
        bad_wr: *mut *mut ibverbs_sys::ibv_recv_wr,
    ) -> ::std::os::raw::c_int;

    fn poll_cq(cq: *mut ibverbs_sys::ibv_cq, num_entries: i32, wc: *mut ibverbs_sys::ibv_wc)
        -> i32;
}

#[repr(C)]
// this struct represent the `bluerdma_device` struct in `bluerdma.h` at `rdma-core/providers/bluerdma/`
// the padding size should match the C's definition.
struct BlueRdmaDevice {
    pad: [u8; 712],
    driver: *mut core::ffi::c_void,
    abi_version: core::ffi::c_int,
}

pub(super) fn get_device(context: *mut ibverbs_sys::ibv_context) -> &'static mut dyn VerbsOps {
    let dev_ptr = unsafe { *context }.device.cast::<BlueRdmaDevice>();
    let driver_ptr = unsafe { (*dev_ptr).driver };
    unsafe {
        #[cfg(feature = "hw")]
        {
            driver_ptr.cast::<HwDeviceCtx<PciHwDevice>>().as_mut()
        }
        #[cfg(feature = "sim")]
        {
            driver_ptr.cast::<HwDeviceCtx<EmulatedHwDevice>>().as_mut()
        }
        #[cfg(feature = "mock")]
        {
            driver_ptr.cast::<MockDeviceCtx>().as_mut()
        }
    }
    .unwrap_or_else(|| unreachable!("null device pointer"))
}
