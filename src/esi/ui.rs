use super::{EsiClient, Result};

/// Where a waypoint goes, and how it joins the route already set.
#[derive(Debug, Clone, Copy)]
pub struct Waypoint {
    /// A solar system, station, or structure id.
    pub destination_id: i64,
    pub add_to_beginning: bool,
    pub clear_other_waypoints: bool,
}

impl EsiClient {
    pub async fn set_waypoint(&self, token: &str, waypoint: Waypoint) -> Result<()> {
        let Waypoint {
            destination_id,
            add_to_beginning,
            clear_other_waypoints,
        } = waypoint;
        // Values are plain ints/bools, so no escaping is needed.
        let path = format!(
            "/ui/autopilot/waypoint?destination_id={destination_id}\
             &add_to_beginning={add_to_beginning}&clear_other_waypoints={clear_other_waypoints}"
        );
        // The waypoint endpoint returns an empty body, so we just check the status.
        self.send_checked(
            self.request(reqwest::Method::POST, &path, Some(token)),
            Some(token),
        )
        .await?;
        Ok(())
    }
}
