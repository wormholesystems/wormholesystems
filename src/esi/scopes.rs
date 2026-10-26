/// An ESI scope the application requests at SSO consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    ReadLocation,
    ReadShipType,
    ReadOnline,
    WriteWaypoint,
    /// Optional: names the structure a pilot is docked in. Never asked for at sign-in.
    ReadStructures,
}

impl Scope {
    pub const ALL: [Scope; 5] = [
        Scope::ReadLocation,
        Scope::ReadShipType,
        Scope::ReadOnline,
        Scope::WriteWaypoint,
        Scope::ReadStructures,
    ];

    /// What sign-in asks for, and what a character is warned about lacking. The rest are
    /// granted on purpose, from the settings page.
    pub const REQUIRED: [Scope; 4] = [
        Scope::ReadLocation,
        Scope::ReadShipType,
        Scope::ReadOnline,
        Scope::WriteWaypoint,
    ];

    pub fn is_optional(self) -> bool {
        !Scope::REQUIRED.contains(&self)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::ReadLocation => "esi-location.read_location.v1",
            Scope::ReadShipType => "esi-location.read_ship_type.v1",
            Scope::ReadOnline => "esi-location.read_online.v1",
            Scope::WriteWaypoint => "esi-ui.write_waypoint.v1",
            Scope::ReadStructures => "esi-universe.read_structures.v1",
        }
    }

    pub fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|scope| scope.as_str() == s)
    }
}
