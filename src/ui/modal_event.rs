pub enum ModalOutcome {
    Consumed,
    Dismissed,
    Confirmed,
    Execute(String),
}
