pub enum ModalOutcome {
    Consumed,
    Dismissed,
    Confirmed,
    Execute(String),
}

pub enum OverlayOutcome {
    /// Overlay should close
    Dismissed,
    /// Key was consumed (scroll etc.)
    Consumed,
    /// Key was not handled; let it fall through to normal handling
    Passthrough,
}

pub enum PanelOutcome {
    /// Navigation key was handled
    Consumed,
    /// Enter in non-action-mode: caller should execute the command line
    ExecuteCommand,
    /// Key is not a panel navigation key; caller should handle it
    Passthrough,
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
