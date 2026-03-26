// // Element : impl EventHandler<Event1> : Event1

// pub enum WifiEvent{
//     WifiOn,
//     WifiOff
// }

// pub enum PointerEvent{
//     Click,
//     Press
// }

// impl EventHandler<WifiEvent>

// // impl Eventimpl for WifiEvent{
//     // fn ``
// // }

trait EventImpl {}

trait EventHandler<T: EventImpl> {
    fn handle_event(&mut self, event: T);
}

// Temp

#[derive(Debug)]
enum Pointer {
    In,
    Out,
    Hover,
    Click,
}

impl EventImpl for Pointer {}

struct Button {
    size: u32 
}

impl EventHandler<Pointer> for Button {
    fn handle_event(&mut self, event: Pointer) {
        match event {
            Pointer::Click => println!("Clicked"),
            _ => println!("{:?}", event)
        }
    }
}
