use super::{Command, WaitingRoom};

pub(super) struct WaitingRoomStep<'a, T, D> {
    waiting_room: &'a mut WaitingRoom<T, D>,
}

impl<'a, T, D> WaitingRoomStep<'a, T, D> {
    pub fn new(waiting_room: &'a mut WaitingRoom<T, D>) -> WaitingRoomStep<'a, T, D> {
        WaitingRoomStep{
            waiting_room
        }
    }

    pub fn tick(self) -> Vec<Command> {
        Vec::new()
    }
}
