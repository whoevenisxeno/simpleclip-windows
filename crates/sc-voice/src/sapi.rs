use crate::hear::{Heard, OPENERS};
use std::path::Path;
use windows::core::{IUnknown, Interface, PCWSTR, PWSTR};
use windows::Win32::Media::Audio::waveInGetNumDevs;
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const COMMAND: u64 = 1;
const JUNK: u64 = 2;
const WHOLE: u32 = u32::MAX;
const HEADS: [&str; 2] = ["simple clip", "simpleclip"];
const TAIL: &str = "that";
const WEAK: f32 = 0.3;
const DICTATION: SPSTATEHANDLE = SPSTATEHANDLE(-3isize as *mut core::ffi::c_void);

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

pub struct Listener {
    _rec: ISpRecognizer,
    ctx: ISpRecoContext,
    _grammars: [ISpRecoGrammar; 2],
}

pub fn init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn has_mic() -> bool {
    unsafe { waveInGetNumDevs() > 0 }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn bit(e: SPEVENTENUM) -> u64 {
    1u64 << e.0
}

unsafe fn take_string(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

unsafe fn language(rec: &ISpRecognizer) -> Option<u16> {
    let token = rec.GetRecognizer().ok()?;
    let attrs = token.OpenKey(windows::core::w!("Attributes")).ok()?;
    let raw = take_string(attrs.GetStringValue(windows::core::w!("Language")).ok()?);
    let first = raw.split(';').next()?.trim();
    u16::from_str_radix(first, 16).ok()
}

unsafe fn command_grammar(
    ctx: &ISpRecoContext,
    lang: Option<u16>,
) -> windows::core::Result<ISpRecoGrammar> {
    let g = ctx.CreateGrammar(COMMAND)?;
    if let Some(l) = lang {
        g.ResetGrammar(l)?;
    }
    let mut top = SPSTATEHANDLE::default();
    let flags = (SPRAF_TopLevel.0 | SPRAF_Active.0) as u32;
    g.GetRule(windows::core::w!("clip"), 1, flags, true, &mut top)?;
    let mut head = SPSTATEHANDLE::default();
    g.CreateNewState(top, &mut head)?;
    let mut mid = SPSTATEHANDLE::default();
    g.CreateNewState(top, &mut mid)?;
    let end = SPSTATEHANDLE::default();
    let word = |from: SPSTATEHANDLE, to: SPSTATEHANDLE, text: Option<&str>, weight: f32| {
        let w = text.map(wide);
        let p = w.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()));
        g.AddWordTransition(
            from,
            to,
            p,
            windows::core::w!(" "),
            SPWT_LEXICAL,
            weight,
            std::ptr::null(),
        )
    };
    for o in OPENERS {
        word(top, head, Some(o), 1.0)?;
    }
    word(top, head, None, 1.0)?;
    for h in HEADS {
        word(head, mid, Some(h), 1.0)?;
    }
    word(mid, end, Some(TAIL), 1.0)?;
    word(mid, end, None, WEAK)?;
    g.AddRuleTransition(mid, end, DICTATION, WEAK, std::ptr::null())?;
    g.Commit(0)?;
    g.SetRuleState(PCWSTR::null(), std::ptr::null_mut(), SPRS_ACTIVE)?;
    Ok(g)
}

unsafe fn junk_grammar(ctx: &ISpRecoContext) -> windows::core::Result<ISpRecoGrammar> {
    let g = ctx.CreateGrammar(JUNK)?;
    g.LoadDictation(PCWSTR::null(), SPLO_STATIC)?;
    g.SetDictationState(SPRS_ACTIVE)?;
    Ok(g)
}

impl Listener {
    pub fn open(src: Source) -> Result<Self, Problem> {
        unsafe { Self::build(src) }
    }

    unsafe fn build(src: Source) -> Result<Self, Problem> {
        let rec: ISpRecognizer = CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL)
            .map_err(|_| Problem::NoSpeech)?;
        match src {
            Source::Mic => {
                if !has_mic() {
                    return Err(Problem::NoMic);
                }
                let audio: IUnknown =
                    CoCreateInstance(&SpMMAudioIn, None, CLSCTX_ALL).map_err(|_| Problem::NoMic)?;
                rec.SetInput(&audio, true).map_err(|_| Problem::NoMic)?;
            }
            Source::File(path) => {
                let stream: ISpStream =
                    CoCreateInstance(&SpStream, None, CLSCTX_ALL).map_err(|_| Problem::NoSpeech)?;
                let w = wide(&path.to_string_lossy());
                stream
                    .BindToFile(PCWSTR(w.as_ptr()), SPFM_OPEN_READONLY, None, None, 0)
                    .map_err(|_| Problem::BadFile)?;
                rec.SetInput(&stream, true).map_err(|_| Problem::BadFile)?;
            }
        }
        let ctx = rec.CreateRecoContext().map_err(|_| Problem::NoSpeech)?;
        let wanted = bit(SPEI_RECOGNITION)
            | bit(SPEI_END_SR_STREAM)
            | bit(SPEI_RESERVED1)
            | bit(SPEI_RESERVED2);
        ctx.SetNotifyWin32Event().map_err(|_| Problem::NoSpeech)?;
        ctx.SetInterest(wanted, wanted)
            .map_err(|_| Problem::NoSpeech)?;
        let lang = language(&rec);
        let command = command_grammar(&ctx, lang).map_err(|_| Problem::NoSpeech)?;
        let junk = junk_grammar(&ctx).map_err(|_| Problem::NoSpeech)?;
        rec.SetRecoState(SPRST_ACTIVE)
            .map_err(|_| Problem::NoSpeech)?;
        Ok(Self {
            _rec: rec,
            ctx,
            _grammars: [command, junk],
        })
    }

    pub fn wait(&self, ms: u32) -> windows::core::Result<Vec<Got>> {
        unsafe {
            self.ctx.WaitForNotifyEvent(ms)?;
            let mut out = Vec::new();
            loop {
                let mut ev = SPEVENT::default();
                let mut n = 0u32;
                self.ctx.GetEvents(1, &mut ev, &mut n)?;
                if n == 0 {
                    break;
                }
                if let Some(g) = read_event(&ev) {
                    out.push(g);
                }
            }
            Ok(out)
        }
    }
}

unsafe fn read_event(ev: &SPEVENT) -> Option<Got> {
    let id = ev._bitfield & 0xffff;
    let kind = (ev._bitfield >> 16) & 0xffff;
    let raw = ev.lParam.0 as *mut core::ffi::c_void;
    let object =
        (kind == SPET_LPARAM_IS_OBJECT.0 && !raw.is_null()).then(|| IUnknown::from_raw(raw));
    if (kind == SPET_LPARAM_IS_POINTER.0 || kind == SPET_LPARAM_IS_STRING.0) && !raw.is_null() {
        CoTaskMemFree(Some(raw));
    }
    if id == SPEI_END_SR_STREAM.0 {
        return Some(Got::End);
    }
    if id != SPEI_RECOGNITION.0 {
        return None;
    }
    let result: ISpRecoResult = object?.cast().ok()?;
    let phrase = result.GetPhrase().ok()?;
    if phrase.is_null() {
        return None;
    }
    let confidence = (*phrase).Base.Rule.SREngineConfidence;
    let command = (*phrase).Base.ullGrammarID == COMMAND;
    let base = &(*phrase).Base;
    let words: Vec<f32> = if base.pElements.is_null() {
        Vec::new()
    } else {
        let first = base.Rule.ulFirstElement as usize;
        let n = base.Rule.ulCountOfElements as usize;
        std::slice::from_raw_parts(base.pElements, first + n)[first..]
            .iter()
            .map(|e| e.SREngineConfidence)
            .collect()
    };
    CoTaskMemFree(Some(phrase as *const _));
    let mut text = PWSTR::null();
    result.GetText(WHOLE, WHOLE, true, &mut text, None).ok()?;
    Some(Got::Heard(Heard {
        text: take_string(text),
        confidence,
        words,
        command,
    }))
}
