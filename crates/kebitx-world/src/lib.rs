//! Minecraft worlds, read with kebitx's own code.
//!
//! `leveldb` reads the database Bedrock keeps a world in, and `bedrock` says what is
//! saved there. Nothing here writes: a world is read, never touched. Every byte of
//! it is untrusted, so damage is counted and reported, never a crash.
#![forbid(unsafe_code)]

pub mod bedrock;
pub mod leveldb;
