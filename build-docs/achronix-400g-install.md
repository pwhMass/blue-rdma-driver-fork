# Achronix 400G 安装（初版）

1. 分支与 BSC  
   `git checkout simple-400g`  
   运行 `setup.sh` 安装 bsc，并在 `~/.bashrc` 中为新增 `export` 补上引号且确保 PATH 追加生效。
需要确认 bsc版本和Ubuntu版本匹配

2. 仿真依赖  
   `sudo apt install iverilog verilator zlib1g-dev`  
   `pip install cocotb cocotb-test cocotbext-pcie cocotbext-axi scapy`
   （目前python 我使用 conda）
3. Backend 构建  
   `sudo apt install tcl8.6 libtcl8.6` 后执行 backend 构建以生成 `verilog`。
4. 运行系统级测试  
   `cd achronix-400g/test/cocotb && make run_system_test_server`
