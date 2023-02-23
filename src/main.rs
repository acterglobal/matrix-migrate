use std::time::Duration;

use clap::Parser;
use futures::{
    future::{join_all, try_join_all},
    pin_mut, try_join, StreamExt,
};
use log::{info, warn};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{OwnedRoomId, OwnedServerName, OwnedUserId},
    Client,
};

/// Fast migration of one matrix account to another
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Username of the account to migrate from
    #[arg(long = "from", env = "FROM_USER")]
    from_user: OwnedUserId,

    /// Password of the account to migrate from
    #[arg(long = "from-pw", env = "FROM_PASSWORD")]
    from_user_password: String,

    /// Custom homeserver, if not defined discovery is used
    #[arg(long, env = "FROM_HOMESERVER")]
    from_homeserver: Option<OwnedServerName>,

    /// Username of the given account to migrate to
    #[arg(long = "to", env = "TO_USER")]
    to_user: OwnedUserId,

    /// Password of the account to migrate from
    #[arg(long = "to-pw", env = "TO_PASSWORD")]
    to_user_password: String,

    /// Custom homeserver, if not defined discovery is used
    #[arg(long, env = "TO_HOMESERVER")]
    to_homeserver: Option<OwnedServerName>,

    /// Custom logging info
    #[arg(long, env = "RUST_LOG", default_value = "matrix_migrate=info")]
    log: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    env_logger::Builder::new().parse_filters(&args.log).init();

    let from_cb = Client::builder().user_agent("matrix-migrate/1");
    let from_c = if let Some(h) = args.from_homeserver {
        from_cb.server_name(&h).build().await?
    } else {
        from_cb
            .server_name(args.from_user.server_name())
            .build()
            .await?
    };

    info!("Logging in {:}", args.from_user);

    from_c
        .login_username(args.from_user, &args.from_user_password)
        .send()
        .await?;

    let to_cb = Client::builder().user_agent("matrix-migrate/1");
    let to_c = if let Some(h) = args.to_homeserver {
        to_cb.server_name(&h).build().await?
    } else {
        to_cb
            .server_name(args.to_user.server_name())
            .build()
            .await?
    };

    info!("Logging in {:}", args.to_user);

    to_c.login_username(args.to_user, &args.to_user_password)
        .send()
        .await?;

    info!("All logged in. Syncing...");

    let to_c_stream = to_c.clone();
    let to_sync_stream = to_c_stream.sync_stream(SyncSettings::default()).await;
    pin_mut!(to_sync_stream);

    try_join!(from_c.sync_once(SyncSettings::default()), async {
        to_sync_stream.next().await.unwrap()
    })?;

    info!("--- Synced");

    let all_prev_rooms = from_c
        .joined_rooms()
        .into_iter()
        .map(|r| r.room_id().to_owned())
        .collect::<Vec<_>>();

    let all_new_rooms = to_c
        .joined_rooms()
        .into_iter()
        .map(|r| r.room_id().to_owned())
        .chain(
            to_c.invited_rooms()
                .into_iter()
                .map(|r| r.room_id().to_owned()),
        )
        .collect::<Vec<_>>();

    let (already_invited, to_invite): (Vec<_>, Vec<_>) = all_prev_rooms
        .iter()
        .partition(|r| all_new_rooms.contains(r));

    let invites_to_accept = to_c
        .invited_rooms()
        .into_iter()
        .filter_map(|r| {
            let room_id = r.room_id().to_owned();
            if all_prev_rooms.contains(&room_id) {
                Some(room_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    info!(
        "--- Already sharing {}; Rooms to accept: {};  Rooms to invite: {}",
        already_invited.len(),
        invites_to_accept.len(),
        to_invite.len()
    );

    let to_user = to_c.user_id().unwrap().to_owned();
    let to_accept = invites_to_accept.iter().collect();
    let c_accept = to_c.clone();
    let ensure_user = to_user.clone();
    let ensure_c = from_c.clone();
    let inviter_c = from_c.clone();

    let (_, not_yet_accepted, remaining_invites) = try_join!(
        async move { ensure_power_levels(&ensure_c, ensure_user, &already_invited).await },
        async move { accept_invites(&c_accept, &to_accept).await },
        async move {
            let to_invite = to_invite.clone();
            send_invites(&inviter_c, &to_invite, to_user.clone()).await?;
            ensure_power_levels(&inviter_c, to_user.clone(), &to_invite).await?;
            Ok(to_invite
                .into_iter()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>())
        },
    )?;

    let mut invites_awaiting = not_yet_accepted
        .into_iter()
        .chain(remaining_invites.into_iter())
        .collect::<Vec<_>>();

    info!("First invitation set done.");
    while invites_awaiting.len() > 0 {
        info!("Still {} rooms to go. Syncing up", invites_awaiting.len());
        to_sync_stream.next().await.expect("Sync stream broke")?;
        invites_awaiting = accept_invites(&to_c, &invites_awaiting.iter().collect()).await?;
    }

    info!("-- All done! -- ");

    Ok(())
}

async fn ensure_power_levels(
    from_c: &Client,
    username: OwnedUserId,
    rooms: &Vec<&OwnedRoomId>,
) -> anyhow::Result<()> {
    Ok(())
    // unimplemented!()
}

async fn accept_invites(
    to_c: &Client,
    rooms: &Vec<&OwnedRoomId>,
) -> anyhow::Result<Vec<OwnedRoomId>> {
    let mut pending = Vec::new();
    for room_id in rooms {
        let Some(invited) = to_c.get_invited_room(&room_id) else {
            if to_c.get_joined_room(room_id).is_some() { // already existing, skipping
                continue
            }
            pending.push(room_id.clone().to_owned());
            continue
        };
        info!(
            "Accepting invite for {}({})",
            invited.display_name().await?,
            invited.room_id()
        );
        invited.accept_invitation().await?;
    }

    Ok(pending)
}

async fn send_invites(
    from_c: &Client,
    rooms: &Vec<&OwnedRoomId>,
    user_id: OwnedUserId,
) -> anyhow::Result<()> {
    join_all(rooms.iter().enumerate().map(|(counter, room_id)| {
        let from_c = from_c.clone();
        let user_id = user_id.clone();
        async move {
            tokio::time::sleep(Duration::from_secs(counter as u64)).await;
            let Some(joined) = from_c.get_joined_room(&room_id) else {
                        warn!("Can't invite user to {:}: not a member myself", room_id);
                        return
                    };
            info!(
                "Inviting to {room_id} ({})",
                joined.display_name().await.unwrap()
            );
            if let Err(e) = joined.invite_user_by_id(&user_id).await {
                warn!("Inviting to {:} failed: {e}", room_id);
            }
        }
    }))
    .await;

    Ok(())
}
