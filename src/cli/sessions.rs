//! Standalone `zakhar sessions` command: list saved chat sessions in the
//! terminal without entering the interactive chat.

use crate::session;

pub fn sessions() {
    println!("{}", session::list_formatted());
}