# Blue RDMA Driver 安装（初版）

1. rust 安装
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

1. Rust 驱动依赖（WSL）  
   `sudo apt install cmake libnl-3-dev libnl-route-3-dev libclang-dev libibverbs-dev`  
   `cd blue-rdma-driver/dtld-ibverbs && cargo build --no-default-features --features sim`
2. 构建 rdma-core-55.0  
   `cd blue-rdma-driver/dtld-ibverbs/rdma-core-55.0 && ./build.sh`
   如果路径太长可能会出现错误

```log
   [ 77%] Building C object infiniband-diags/CMakeFiles/perfquery.dir/perfquery.c.o
[ 77%] Linking C executable ../bin/perfquery
[ 77%] Built target perfquery
[ 77%] Building C object infiniband-diags/CMakeFiles/saquery.dir/saquery.c.o
[ 77%] Linking C executable ../bin/saquery
[ 77%] Built target saquery
[ 77%] Building C object infiniband-diags/CMakeFiles/sminfo.dir/sminfo.c.o
[ 77%] Linking C executable ../bin/sminfo
[ 77%] Built target sminfo
[ 78%] Building C object infiniband-diags/CMakeFiles/smpdump.dir/smpdump.c.o
[ 78%] Linking C executable ../bin/smpdump
[ 78%] Built target smpdump
[ 78%] Building C object infiniband-diags/CMakeFiles/smpquery.dir/smpquery.c.o
[ 79%] Linking C executable ../bin/smpquery
[ 79%] Built target smpquery
[ 80%] Building C object infiniband-diags/CMakeFiles/vendstat.dir/vendstat.c.o
[ 80%] Linking C executable ../bin/vendstat
[ 80%] Built target vendstat
[ 80%] Building C object infiniband-diags/CMakeFiles/ibsendtrap.dir/ibsendtrap.c.o
[ 80%] Linking C executable ../bin/ibsendtrap
[ 80%] Built target ibsendtrap
[ 80%] Building C object infiniband-diags/CMakeFiles/mcm_rereg_test.dir/mcm_rereg_test.c.o
[ 80%] Linking C executable ../bin/mcm_rereg_test
[ 80%] Built target mcm_rereg_test
[ 81%] Building C object ibacm/CMakeFiles/ibacm.dir/src/acm.c.o
In file included from /home/peng/projects/rdma_all/blue-rdma-driver-fork/dtld-ibverbs/rdma-core-55.0/build/include/ccan/minmax.h:7,
                 from /home/peng/projects/rdma_all/blue-rdma-driver-fork/dtld-ibverbs/rdma-core-55.0/ibacm/linux/osd.h:48,
                 from /home/peng/projects/rdma_all/blue-rdma-driver-fork/dtld-ibverbs/rdma-core-55.0/ibacm/src/acm.c:38:
/home/peng/projects/rdma_all/blue-rdma-driver-fork/dtld-ibverbs/rdma-core-55.0/ibacm/src/acm.c: In function ‘acm_listen’:
/home/peng/projects/rdma_all/blue-rdma-driver-fork/dtld-ibverbs/rdma-core-55.0/build/include/ccan/build_assert.h:23:33: error: size of unnamed array is negative
   23 | do { (void) sizeof(char [1 - 2*!(cond)]); } while(0)
      |                         ^

/home/peng/projects/rdma_all/blue-rdma-driver-fork/dtld-ibverbs/rdma-core-55.0/ibacm/src/acm.c:628:17: note: in expansion of macro ‘BUILD_ASSERT’
  628 |                 BUILD_ASSERT(sizeof(IBACM_IBACME_SERVER_PATH) <=
      |                 ^~~~~~~~~~~~
make[2]: *** [ibacm/CMakeFiles/ibacm.dir/build.make:76: ibacm/CMakeFiles/ibacm.dir/src/acm.c.o] Error 1
make[1]: *** [CMakeFiles/Makefile2:2672: ibacm/CMakeFiles/ibacm.dir/all] Error 2
make: *** [Makefile:136: all] Error 2
```


3. 准备 WSL2 内核头（内核 6.6.87.2）  
   `sudo apt install build-essential flex bison dwarves libssl-dev libelf-dev cpio bc kmod`  
   `KERNEL_SRC=/path/to/WSL2-Linux-Kernel`  
   `cd "$KERNEL_SRC" && sudo make KCONFIG_CONFIG=Microsoft/config-wsl modules_prepare -j$(nproc)`  
   `cd "$KERNEL_SRC" && sudo make KCONFIG_CONFIG=Microsoft/config-wsl modules -j$(nproc)`  
   `sudo ln -s "$KERNEL_SRC" /lib/modules/$(uname -r)/build`
4. 编译驱动  
   `cd blue-rdma-driver && git submodule update --init --recursive`  
   `make clean && make`（缺少 BTF 时可 `make KBUILD_MODPOST_WARN=1`）
5. 配置 loopback  
   `sudo ip addr add 17.34.51.10/24 dev blue0`  
   `sudo ip addr add 17.34.51.11/24 dev blue1`
6. 大页准备