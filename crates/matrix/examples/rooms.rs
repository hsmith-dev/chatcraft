//! Diagnostic: what the SDK's store believes about an account's rooms.
//! `cargo run -p chatcraft-matrix --example rooms -- <homeserver> <user> <store-dir>`
use matrix_sdk::Client;
use matrix_sdk::authentication::matrix::MatrixSession;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (hs, dir) = (a.first().cloned().unwrap_or_default(), std::path::PathBuf::from(a.get(2).cloned().unwrap_or_default()));
    let pass = std::fs::read_to_string(dir.join("passphrase"))?;
    let client = Client::builder().server_name_or_homeserver_url(&hs).sqlite_store(dir.join("store"), Some(pass.trim())).build().await?;
    let session: MatrixSession = serde_json::from_str(&std::fs::read_to_string(dir.join("session.json"))?)?;
    client.restore_session(session).await?;
    let r = client.sync_once(matrix_sdk::config::SyncSettings::default().timeout(std::time::Duration::from_secs(0))).await?;
    println!("response: joined {:?} invited {:?}", r.rooms.joined.keys().collect::<Vec<_>>(), r.rooms.invited.keys().collect::<Vec<_>>());
    for room in client.rooms() {
        println!("store: {} state={:?} direct={:?}", room.room_id(), room.state(), room.is_direct().await);
        for m in room.members(matrix_sdk::RoomMemberships::JOIN).await.unwrap_or_default() {
            println!("  member {} display_name={:?} name={:?}", m.user_id(), m.display_name(), m.name());
        }
    }
    Ok(())
}
