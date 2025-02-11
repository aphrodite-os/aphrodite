//! Functions to output to various things
#![cfg(any(target_arch = "x86"))]

use super::ports;
use paste::paste;

macro_rules! message_funcs {
    ($func_name:ident, $prefix:literal, $level:ident) => {
        paste! {
            /// Outputs a $func_name message &str to the debug serial port.
            pub fn [< s $func_name s >](s: &str) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, $prefix.as_bytes());
                ports::outbs(super::DEBUG_PORT, s.as_bytes());
            }
            /// Outputs a $func_name message &str and a newline to the debug serial port.
            pub fn [< s $func_name sln >](s: &str) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, $prefix.as_bytes());
                ports::outbs(super::DEBUG_PORT, s.as_bytes());
                ports::outb(super::DEBUG_PORT, b'\n');
            }

            /// Outputs a $func_name message &\[u8] to the debug serial port.
            pub fn [< s $func_name b >](s: &[u8]) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, $prefix.as_bytes());
                ports::outbs(super::DEBUG_PORT, s);
            }
            /// Outputs a $func_name message &\[u8] and a newline to the debug serial port.
            pub fn [< s $func_name bln >](s: &[u8]) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, $prefix.as_bytes());
                ports::outbs(super::DEBUG_PORT, s);
                ports::outb(super::DEBUG_PORT, b'\n');
            }

            /// Outputs a(n) $func_name message u8 to the debug serial port.
            pub fn [< s $func_name u >](s: u8) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, $prefix.as_bytes());
                ports::outb(super::DEBUG_PORT, s);
            }

            ///////////////////////////////////////////////////////////////

            /// Outputs a $func_name message &str to the debug serial port without a prefix.
            pub fn [< s $func_name snp >](s: &str) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, s.as_bytes());
            }
            /// Outputs a $func_name message &str and a newline to the debug serial port without a prefix.
            pub fn [< s $func_name snpln >](s: &str) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, s.as_bytes());
                ports::outb(super::DEBUG_PORT, b'\n');
            }

            /// Outputs a $func_name message &\[u8] to the debug serial port without a prefix.
            pub fn [< s $func_name bnp >](s: &[u8]) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, s);
            }
            /// Outputs a $func_name message &\[u8] and a newline to the debug serial port without a prefix.
            pub fn [< s $func_name bnpln >](s: &[u8]) {
                if cfg!($level = "false") {
                    return
                }
                ports::outbs(super::DEBUG_PORT, s);
                ports::outb(super::DEBUG_PORT, b'\n');
            }

            /// Outputs a(n) $func_name message u8 to the debug serial port without a prefix.
            pub fn [< s $func_name unp >](s: u8) {
                if cfg!($level = "false") {
                    return
                }
                ports::outb(super::DEBUG_PORT, s);
            }
        }
    }
}

message_funcs!(debug, "[DEBUG] ", CONFIG_PREUSER_OUTPUT_DEBUG);
message_funcs!(info, "[INFO] ", CONFIG_PREUSER_OUTPUT_INFO);
message_funcs!(warning, "[WARN] ", CONFIG_PREUSER_OUTPUT_WARN);
message_funcs!(error, "[ERROR] ", CONFIG_PREUSER_OUTPUT_ERROR);
message_funcs!(fatal, "[FATAL] ", CONFIG_PREUSER_OUTPUT_FATAL);
message_funcs!(output, "", NONE);

