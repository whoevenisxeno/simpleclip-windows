#![allow(dead_code)]

use crate::hear::Heard;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    NoSpeech,
    NoMic,
    BadFile,
}

pub enum Got {
    Heard(Heard),
    End,
}

pub enum Source<'a> {
    Mic,
    File(&'a Path),
}

pub struct Listener;

pub fn init() {}

pub fn has_mic() -> bool {
    false
}

impl Listener {
    pub fn open(_src: Source) -> Result<Self, Problem> {
        Err(Problem::NoSpeech)
    }

    pub fn wait(&self, _ms: u32) -> Result<Vec<Got>, ()> {
        Ok(Vec::new())
    }
}
