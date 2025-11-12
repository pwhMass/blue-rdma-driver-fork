# SPDX-License-Identifier: GPL-2.0
KERNEL_DIR ?= /lib/modules/$(shell uname -r)/build

obj-m := bluerdma.o

bluerdma-y := \
	main.o \
	verbs.o \
	ethernet.o

# Use Mellanox OFED kernel headers for compilation
OFED_DIR := /usr/src/mlnx-ofed-kernel-24.10.OFED.24.10.1.1.4.1
ccflags-y += -I$(OFED_DIR)/include
ccflags-y += -I$(OFED_DIR)/include/rdma
ccflags-y += -I$(OFED_DIR)/include/uapi

# IMPORTANT: Use OFED Module.symvers for symbol CRC calculation
ARCH := $(shell uname -m)
KVER := $(shell uname -r)
OFED_SYMVERS := /usr/src/ofa_kernel/$(ARCH)/$(KVER)/Module.symvers

test: bluerdma.ko
	sudo /usr/src/linux-headers-$(shell uname -r)/scripts/sign-file sha256 /var/lib/shim-signed/mok/MOK.priv /var/lib/shim-signed/mok/MOK.der $^
	sudo insmod $^
	ls /dev/infiniband

bluerdma.ko: main.c verbs.c ethernet.c
	$(MAKE) -C $(KERNEL_DIR) M=$(PWD) KBUILD_EXTRA_SYMBOLS=$(OFED_SYMVERS) modules

qemu: main.c verbs.c ethernet.c
	$(MAKE) -C /linux-src M=$(PWD)

clean:
	$(MAKE) -C $(KERNEL_DIR) M=$(PWD) clean
	rm -f *.o *.ko *.mod.* modules.order Module.symvers .*.cmd

.PHONY: test clean qemu
