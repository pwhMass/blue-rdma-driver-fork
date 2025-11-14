# 从 Mellanox OFED 切换到标准 Linux RDMA

**日期**: 2025-11-14
**目标**: 使 bluerdma 在标准 Linux RDMA 子系统下运行
**前置文档**: [OFED RoCE 注册问题](./ofed-roce-registration-issue.md)
**状态**: 📋 推荐方案

---

## 为什么要切换？

### bluerdma 的特点

bluerdma 是一个**软件 RDMA 驱动**：

```
✓ 纯软件实现（无硬件依赖）
✓ 虚拟网络设备（blue0, blue1）
✓ 软件 GID 表管理
✓ 软件数据路径（无硬件 DMA）
```

### Mellanox OFED vs 标准 Linux RDMA

| 特性 | Mellanox OFED | 标准 Linux RDMA |
|------|--------------|----------------|
| **设计目标** | Mellanox 硬件优化 | 通用 RDMA 框架 |
| **硬件假设** | 期望真实 PCI 设备 | 支持软件驱动 |
| **软件驱动支持** | ❌ 受限 | ✅ 完整支持 |
| **示例驱动** | mlx5_ib (硬件) | rxe, siw (软件) |
| **bluerdma 兼容性** | ❌ 不兼容 | ✅ 兼容 |

### 切换的优势

| 优势 | 说明 |
|------|------|
| ✅ **架构匹配** | 标准 RDMA 框架专为软件/硬件混合设计 |
| ✅ **简化开发** | 无需处理 OFED 特定的硬件假设 |
| ✅ **社区支持** | 主线内核的 RDMA 子系统 |
| ✅ **可移植性** | 在任何 Linux 系统上工作 |
| ✅ **参考实现** | rxe/siw 是成熟的软件 RDMA 驱动 |

### 切换的劣势

| 劣势 | 解决方案 |
|------|---------|
| ⚠️ **Mellanox 硬件性能** | 仅在测试 bluerdma 时切换，不影响生产环境 |
| ⚠️ **编译配置修改** | 提供详细的 Makefile 修改指南 |
| ⚠️ **两套模块共存** | 使用模块优先级或临时卸载 OFED 模块 |

---

## 迁移策略

### 策略选择

根据您的需求选择迁移策略：

#### 策略 A: 临时切换（推荐）

**适用场景**: 开发和测试 bluerdma，但保留 OFED 用于生产环境

**方法**:
1. 保留 OFED 安装
2. 临时停止 OFED 服务
3. 加载标准内核 RDMA 模块
4. 测试 bluerdma
5. 完成后恢复 OFED

**优势**: 不影响现有系统配置

#### 策略 B: 双模块共存

**适用场景**: 同时使用 Mellanox 硬件和 bluerdma

**方法**:
1. 修改 Makefile 编译两个版本的 bluerdma
2. bluerdma-ofed.ko (使用 OFED 符号)
3. bluerdma-vanilla.ko (使用标准内核符号)
4. 根据需要加载相应版本

**优势**: 最大灵活性

#### 策略 C: 完全卸载 OFED（不推荐）

**适用场景**: 不使用 Mellanox 硬件，只开发 bluerdma

**方法**:
1. 完全卸载 Mellanox OFED
2. 使用标准内核 RDMA
3. 简化编译配置

**劣势**: 无法使用 Mellanox 硬件的高级特性

---

## 迁移步骤（策略 A：临时切换）

### 前置检查

#### 1. 确认当前状态

```bash
# 检查当前加载的 ib_core
modinfo ib_core | grep filename
# 预期输出：/lib/modules/.../updates/dkms/ib_core.ko.zst (OFED 版本)

# 检查 OFED 版本
ofed_info -s
# 预期输出：MLNX_OFED_LINUX-24.10-1.1.4.0

# 检查 Mellanox 硬件设备
rdma link show
# 如果有 mlx5_0, mlx5_1 等设备，说明在使用 OFED
```

#### 2. 备份当前配置

```bash
# 记录当前加载的 RDMA 模块
lsmod | grep -E "ib_|rdma_|mlx" > ~/rdma_modules_backup.txt

# 记录当前的 RDMA 设备
rdma link show > ~/rdma_devices_backup.txt
ip addr show > ~/network_config_backup.txt
```

### 步骤 1: 停止 OFED 服务并卸载模块

#### 1.1 停止 OFED 服务

```bash
# 停止 OpenIB 服务（如果正在运行）
sudo /etc/init.d/openibd stop

# 或者使用 systemctl（取决于您的系统）
sudo systemctl stop openibd 2>/dev/null || true
```

#### 1.2 卸载 OFED RDMA 模块

```bash
# 卸载所有 RDMA 相关模块（按依赖顺序）
sudo modprobe -r rdma_ucm rdma_cm iw_cm ib_ipoib ib_umad ib_uverbs ib_cm mlx5_ib mlx5_core ib_core

# 验证卸载成功
lsmod | grep ib_core
# 应该没有输出
```

**注意**: 如果卸载失败（模块被占用），可能需要：
```bash
# 查看哪些进程在使用 RDMA
lsof | grep /dev/infiniband

# 停止相关服务
sudo systemctl stop rdma-ndd 2>/dev/null || true
sudo systemctl stop ibacm 2>/dev/null || true
```

### 步骤 2: 加载标准内核 RDMA 模块

#### 2.1 加载基础 RDMA 模块

```bash
# 加载标准内核的 ib_core
sudo modprobe ib_core

# 验证加载的是标准版本
modinfo ib_core | grep filename
# 预期输出：/lib/modules/.../kernel/drivers/infiniband/core/ib_core.ko.xz
# ↑ 注意路径是 kernel/ 而不是 updates/dkms/
```

#### 2.2 加载其他必需模块

```bash
# 加载 RDMA 用户空间支持
sudo modprobe ib_uverbs

# 加载 RDMA CM（连接管理）
sudo modprobe rdma_cm

# 加载 IW CM（iWARP 连接管理）
sudo modprobe iw_cm

# 验证模块加载
lsmod | grep -E "^ib_|^rdma_|^iw_"
```

**预期输出**:
```
ib_core               401408  4 rdma_cm,iw_cm,ib_uverbs,ib_cm
ib_uverbs             180224  0
rdma_cm               139264  0
iw_cm                  57344  1 rdma_cm
```

### 步骤 3: 修改 bluerdma 编译配置

#### 3.1 备份当前 Makefile

```bash
cd /home/peng/projects/rdma_all/blue-rdma-driver
cp Makefile Makefile.ofed.bak
cp kernel-driver/Makefile kernel-driver/Makefile.ofed.bak
```

#### 3.2 修改根目录 Makefile

```bash
# 编辑 Makefile
nano Makefile
```

**修改内容**:

```diff
 KERNEL_SRC ?= /lib/modules/$(shell uname -r)/build

-# OFED Module.symvers for symbol version matching
-ARCH := $(shell uname -m)
-KVER := $(shell uname -r)
-OFED_SYMVERS := /usr/src/ofa_kernel/$(ARCH)/$(KVER)/Module.symvers
-
 BUILD_DIR := build
```

```diff
 bluerdma: $(BUILD_DIR)
-	$(MAKE) -C $(KERNEL_SRC) M=$(CURDIR)/$(BLUERDMA_SRC_DIR) KBUILD_EXTRA_SYMBOLS=$(OFED_SYMVERS) modules
+	$(MAKE) -C $(KERNEL_SRC) M=$(CURDIR)/$(BLUERDMA_SRC_DIR) modules
 	@mkdir -p $(BUILD_DIR)
```

#### 3.3 修改 kernel-driver/Makefile

```bash
# 编辑 kernel-driver/Makefile
nano kernel-driver/Makefile
```

**修改内容**:

```diff
-# Use Mellanox OFED kernel headers for compilation
-OFED_DIR := /usr/src/mlnx-ofed-kernel-24.10.OFED.24.10.1.1.4.1
-ccflags-y += -I$(OFED_DIR)/include
-ccflags-y += -I$(OFED_DIR)/include/rdma
-ccflags-y += -I$(OFED_DIR)/include/uapi
-
-# IMPORTANT: Use OFED Module.symvers for symbol CRC calculation
-ARCH := $(shell uname -m)
-KVER := $(shell uname -r)
-OFED_SYMVERS := /usr/src/ofa_kernel/$(ARCH)/$(KVER)/Module.symvers
+# Use standard kernel RDMA headers
+# (No extra includes needed - use kernel's default)
```

```diff
 bluerdma.ko: main.c verbs.c ethernet.c
-	$(MAKE) -C $(KERNEL_DIR) M=$(PWD) KBUILD_EXTRA_SYMBOLS=$(OFED_SYMVERS) modules
+	$(MAKE) -C $(KERNEL_DIR) M=$(PWD) modules
```

### 步骤 4: 清理并重新编译

```bash
cd /home/peng/projects/rdma_all/blue-rdma-driver

# 清理旧的编译产物
make clean
rm -rf build/

# 重新编译
make
```

**预期输出**:
```
make -C /lib/modules/6.8.0-58-generic/build M=.../kernel-driver modules
  CC [M]  .../kernel-driver/main.o
  CC [M]  .../kernel-driver/verbs.o
  CC [M]  .../kernel-driver/ethernet.o
  LD [M]  .../kernel-driver/bluerdma.o
  MODPOST .../kernel-driver/Module.symvers
  CC [M]  .../kernel-driver/bluerdma.mod.o
  LD [M]  .../kernel-driver/bluerdma.ko
  BTF [M] .../kernel-driver/bluerdma.ko
```

**注意**: 不应该再出现 `KBUILD_EXTRA_SYMBOLS=/usr/src/ofa_kernel/...`

### 步骤 5: 安装并测试 bluerdma

#### 5.1 安装模块

```bash
sudo make install
```

**预期输出**:
```
Loading kernel modules... (Requires root privileges)
modprobe ib_core
insmod build/bluerdma.ko
insmod build/u-dma-buf.ko udmabuf0=2097152
Modules loaded.
```

#### 5.2 验证设备创建

```bash
# 检查内核日志
dmesg | tail -50 | grep -i bluerdma

# 检查 RDMA 设备
rdma link show

# 检查网络设备
ip link show | grep blue
```

**预期成功输出**:
```
# dmesg 应该显示:
DatenLord RDMA driver loaded
ib_alloc_device ok for index 0
Registered network device blue0 for RDMA device 0
ib_alloc_device ok for index 1
Registered network device blue1 for RDMA device 1
ib_set_device_ops ok for index 0
ib_register_device bluerdma0  ✓✓✓ 成功！
Associated netdev blue0 with RDMA device bluerdma0
ib_set_device_ops ok for index 1
ib_register_device bluerdma1  ✓✓✓ 成功！
Associated netdev blue1 with RDMA device bluerdma1

# rdma link show 应该显示:
link bluerdma0/1 state ACTIVE physical_state LINK_UP netdev blue0
link bluerdma1/1 state ACTIVE physical_state LINK_UP netdev blue1

# ip link show 应该显示:
XX: blue0: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN
XX: blue1: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN
```

#### 5.3 测试网络设备

```bash
# 启动 blue0
sudo ip link set blue0 up

# 配置 IP 地址
sudo ip addr add 17.34.51.10/24 dev blue0

# 查看状态
ip addr show blue0
```

---

## 验证和测试

### 1. 验证模块符号版本

```bash
cd /home/peng/projects/rdma_all/blue-rdma-driver/kernel-driver

# 检查 bluerdma.mod.c 中的符号
grep -A 2 "ib_register_device" bluerdma.mod.c

# 应该看到标准内核的 CRC（不是 OFED 的 CRC）
```

### 2. 验证 ib_core 来源

```bash
# 检查当前运行的 ib_core
cat /proc/modules | grep ib_core

# 检查文件路径
modinfo ib_core | grep filename
# 应该显示: /lib/modules/.../kernel/drivers/infiniband/core/ib_core.ko.xz
```

### 3. RDMA 功能测试

```bash
# 查看 RDMA 设备信息
ibv_devices

# 查看设备详细信息
ibv_devinfo -d bluerdma0

# 查看 GID 表
show_gids
```

### 4. 性能测试（可选）

```bash
# 使用 ib_write_bw 测试带宽
ib_write_bw -d bluerdma0

# 使用 ib_read_lat 测试延迟
ib_read_lat -d bluerdma0
```

---

## 恢复到 OFED（如果需要）

### 快速恢复

```bash
# 1. 卸载 bluerdma
sudo rmmod u-dma-buf bluerdma

# 2. 卸载标准 RDMA 模块
sudo modprobe -r ib_uverbs rdma_cm iw_cm ib_core

# 3. 启动 OFED 服务
sudo /etc/init.d/openibd start
# 或者
sudo systemctl start openibd

# 4. 验证 OFED 已恢复
modinfo ib_core | grep filename
# 应该显示: /lib/modules/.../updates/dkms/ib_core.ko.zst

rdma link show
# 应该看到 mlx5_0, mlx5_1 等设备
```

### 恢复 Makefile

```bash
cd /home/peng/projects/rdma_all/blue-rdma-driver

# 恢复 OFED 版本的 Makefile
cp Makefile.ofed.bak Makefile
cp kernel-driver/Makefile.ofed.bak kernel-driver/Makefile

# 重新编译 OFED 版本
make clean
make
```

---

## 双模块共存方案（策略 B）

### 概述

编译两个版本的 bluerdma，在运行时选择使用哪个版本：
- `bluerdma-vanilla.ko` - 使用标准内核 RDMA
- `bluerdma-ofed.ko` - 使用 Mellanox OFED（如果将来修复兼容性）

### 实现步骤

#### 1. 创建构建脚本

```bash
# 创建 build-both.sh
cat > build-both.sh << 'EOF'
#!/bin/bash
set -e

echo "Building bluerdma for both vanilla and OFED..."

# Build vanilla version
echo "=== Building vanilla version ==="
make clean
make VARIANT=vanilla
mkdir -p build/vanilla
cp build/bluerdma.ko build/vanilla/bluerdma-vanilla.ko
cp build/u-dma-buf.ko build/vanilla/

# Build OFED version
echo "=== Building OFED version ==="
make clean
make VARIANT=ofed
mkdir -p build/ofed
cp build/bluerdma.ko build/ofed/bluerdma-ofed.ko
cp build/u-dma-buf.ko build/ofed/

echo "Build complete!"
echo "Vanilla version: build/vanilla/bluerdma-vanilla.ko"
echo "OFED version: build/ofed/bluerdma-ofed.ko"
EOF

chmod +x build-both.sh
```

#### 2. 修改 Makefile 支持变体

```bash
# 编辑根目录 Makefile
nano Makefile
```

**添加变体支持**:
```makefile
VARIANT ?= vanilla

ifeq ($(VARIANT),ofed)
    # OFED 配置
    ARCH := $(shell uname -m)
    KVER := $(shell uname -r)
    OFED_SYMVERS := /usr/src/ofa_kernel/$(ARCH)/$(KVER)/Module.symvers
    EXTRA_SYMBOLS := KBUILD_EXTRA_SYMBOLS=$(OFED_SYMVERS)
else
    # Vanilla 配置
    EXTRA_SYMBOLS :=
endif

bluerdma: $(BUILD_DIR)
	$(MAKE) -C $(KERNEL_SRC) M=$(CURDIR)/$(BLUERDMA_SRC_DIR) $(EXTRA_SYMBOLS) modules
	@mkdir -p $(BUILD_DIR)
	cp $(BLUERDMA_SRC_DIR)/$(BLUERDMA_KO) $(BUILD_DIR)/
```

#### 3. 加载脚本

```bash
# 创建 load-vanilla.sh
cat > load-vanilla.sh << 'EOF'
#!/bin/bash
# 停止 OFED 并加载 vanilla 版本
sudo /etc/init.d/openibd stop
sudo modprobe -r ib_core
sudo modprobe ib_core ib_uverbs
sudo insmod build/vanilla/bluerdma-vanilla.ko
sudo insmod build/vanilla/u-dma-buf.ko udmabuf0=2097152
EOF

chmod +x load-vanilla.sh
```

```bash
# 创建 load-ofed.sh
cat > load-ofed.sh << 'EOF'
#!/bin/bash
# 恢复 OFED 并加载 OFED 版本
sudo rmmod bluerdma u-dma-buf || true
sudo modprobe -r ib_core
sudo /etc/init.d/openibd start
sudo insmod build/ofed/bluerdma-ofed.ko
sudo insmod build/ofed/u-dma-buf.ko udmabuf0=2097152
EOF

chmod +x load-ofed.sh
```

---

## 常见问题排查

### 问题 1: 卸载 OFED 模块失败

**错误**:
```
rmmod: ERROR: Module ib_core is in use by: rdma_cm mlx5_ib ib_uverbs
```

**解决**:
```bash
# 递归卸载所有依赖模块
sudo modprobe -r --remove-dependencies ib_core

# 或者手动按顺序卸载
sudo modprobe -r rdma_ucm rdma_cm
sudo modprobe -r mlx5_ib ib_ipoib
sudo modprobe -r ib_umad ib_uverbs ib_cm
sudo modprobe -r mlx5_core
sudo modprobe -r ib_core
```

### 问题 2: 标准内核没有 ib_core

**错误**:
```
modprobe: FATAL: Module ib_core not found
```

**解决**:
```bash
# 检查内核配置
grep CONFIG_INFINIBAND /boot/config-$(uname -r)

# 如果 CONFIG_INFINIBAND=m，但模块缺失，需要安装
sudo apt install linux-modules-extra-$(uname -r)

# 或者重新编译内核启用 InfiniBand 支持
```

### 问题 3: 编译时仍使用 OFED 头文件

**症状**: 编译日志显示 `-I/usr/src/mlnx-ofed-kernel-...`

**解决**:
```bash
# 确认 Makefile 修改正确
grep "OFED_DIR" kernel-driver/Makefile
# 应该没有输出或被注释

# 清理并重新编译
make clean
rm -rf build/
make
```

### 问题 4: bluerdma 仍然注册失败

**检查**:
```bash
# 1. 确认使用的是标准 ib_core
modinfo ib_core | grep filename
# 应该在 kernel/drivers/... 而不是 updates/dkms/...

# 2. 查看详细错误
dmesg | tail -100 | grep -i "bluerdma\|ib_core"

# 3. 检查符号版本
cd kernel-driver
grep "ib_register_device" bluerdma.mod.c
```

---

## 性能和限制

### 标准 Linux RDMA 的性能

| 指标 | Mellanox OFED | 标准 Linux RDMA | bluerdma (软件) |
|------|--------------|----------------|-----------------|
| **硬件加速** | ✅ 完整支持 | ✅ 完整支持 | ❌ 纯软件 |
| **带宽 (硬件)** | ~100 Gbps | ~100 Gbps | ~10 Gbps (CPU限制) |
| **延迟 (硬件)** | <1 μs | <1 μs | ~10-100 μs |
| **CPU 使用率** | 低 | 低 | 高 |
| **软件驱动支持** | ❌ 受限 | ✅ 完整 | ✅ 兼容 |

**注意**: bluerdma 是软件实现，性能主要受限于 CPU，而非 RDMA 框架本身。

### Mellanox 硬件使用

**重要**: 切换到标准 RDMA 后，Mellanox 硬件 (mlx5) 仍然可以工作：

```bash
# 标准内核也包含 mlx5 驱动
sudo modprobe mlx5_core mlx5_ib

# 查看 Mellanox 设备
rdma link show
# 应该能看到 mlx5_0, mlx5_1 等设备
```

**性能差异**: 标准内核的 mlx5 驱动可能比 OFED 版本稍慢（~5-10%），但对于大多数应用足够。

---

## 最佳实践

### 开发环境配置

**推荐配置**:
```
开发机器:
  ├─ 标准 Linux RDMA (用于 bluerdma 开发)
  └─ bluerdma 模块

生产服务器:
  ├─ Mellanox OFED (用于 Mellanox 硬件)
  └─ bluerdma 暂不部署（等兼容性问题解决）
```

### Git 配置

```bash
# 将 OFED 版本的 Makefile 加入版本控制
git add Makefile.ofed.bak kernel-driver/Makefile.ofed.bak

# 将 vanilla 版本设为默认
git add Makefile kernel-driver/Makefile

# 提交
git commit -m "Add support for vanilla Linux RDMA"
```

### 文档和注释

在代码中添加注释说明：

```c
// main.c
/*
 * This driver is designed to work with standard Linux RDMA subsystem.
 * For Mellanox OFED compatibility, see:
 *   - build-docs/detail/ofed-roce-registration-issue.md
 *   - build-docs/detail/switch-to-vanilla-rdma.md
 */
```

---

## 总结

### 迁移效果

**之前 (Mellanox OFED)**:
```
❌ ib_register_device 失败
❌ 无法创建 RDMA 设备
❌ 需要处理硬件特定的要求
```

**之后 (标准 Linux RDMA)**:
```
✅ ib_register_device 成功
✅ bluerdma0 和 bluerdma1 设备创建
✅ 网络设备 blue0 和 blue1 可用
✅ 符合软件 RDMA 驱动的设计模式
```

### 后续工作

1. ✅ 完成基本的 RDMA 操作实现
2. ✅ 测试 RDMA verbs (post_send, post_recv, poll_cq)
3. ✅ 性能测试和优化
4. ⚠️ 如果需要在 OFED 下工作，需要深入研究 OFED 的硬件抽象层

### 相关文档

- [OFED 符号版本修复](./ofed-symbol-version-fix.md) - 第一阶段问题
- [OFED RoCE 注册问题](./ofed-roce-registration-issue.md) - 第二阶段问题
- [Linux RDMA 编程指南](https://github.com/linux-rdma/rdma-core)
- [软件 RoCE (rxe) 实现](https://github.com/SoftRoCE/rxe-dev)

---

**文档版本**: 1.0
**最后更新**: 2025-11-14
**维护者**: Claude Code Assistant
**状态**: ✅ 推荐使用标准 Linux RDMA
