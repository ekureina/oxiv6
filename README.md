# Oxiv6

Toy RISCV SBI Kernel Written in Rust, Based on Unix v6 / xv6.

Oxiv6 is a kernel written in nightly Rust, licensed under the Apache 2.0 license.

Oxiv6 is not meant to replace a production Operating System / Kernel like Hurd, Linux, BSD, or Redox.
Instead, it is meant as a toy kernel, similar to MIT's teaching xv6-riscv kernel, or my hybrid C / Rust
variation of xv6, [rv6](https://github.com/ekureina/rv6-riscv-ekureina). Unlike xv6 or rv6, however,
Oxiv6 is written in pure rust, with assembly as needed. It also runs on top of RISCV's SBI protocol, and
boots as the payload from an implementation such as OpenSBI (bundled with QEMU).
