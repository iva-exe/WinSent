//! core-types — sdílené datové typy celého workspace.
//!
//! Pravidlo: tahle crate nesmí záviset na ničem windowsovém. Typy sem
//! přibývají průběžně s verzemi (v0 má jen config a IPC ping).

pub mod action;
pub mod config;
pub mod ipc;
pub mod proc;
