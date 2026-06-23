pub enum ModalOutcome {
    Consumed,
    Dismissed,
    Confirmed,
    Execute(String),
}

pub enum PopupOutcome {
    /// Key consumed by the popup (navigation etc.)
    Consumed,
    /// Close the popup; key is fully handled
    Dismissed,
    /// Selected item text; what to do with it is caller-specific
    Accept(String),
    /// Insert this char into the cmdline, then refresh the popup
    InsertChar(char),
    /// Delete one char from the cmdline, then refresh the popup
    Backspace,
    /// Close the popup and let the key fall through to normal handling
    Passthrough,
}
