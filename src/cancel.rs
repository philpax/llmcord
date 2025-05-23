use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, EditMessage, Http, Message, MessageId, UserId,
};

pub const CANCEL_ID_BASE: &str = "cancel";

/// Builds a cancel button message ID from a message ID and a user ID.
pub fn build_id(first_id: MessageId, user_id: UserId) -> String {
    format!("{CANCEL_ID_BASE}#{first_id}#{user_id}")
}

/// Parses a cancel button message ID into a message ID and a user ID.
pub fn parse_id(id: &str) -> Option<(MessageId, UserId)> {
    let mut split_id = id.split('#');
    if split_id.next() != Some(CANCEL_ID_BASE) {
        return None;
    }
    Some((
        MessageId::new(split_id.next()?.parse::<u64>().ok()?),
        UserId::new(split_id.next()?.parse::<u64>().ok()?),
    ))
}

/// Adds a cancel button to a message.
pub async fn add_button(
    http: &Http,
    first_id: MessageId,
    msg: &mut Message,
    user_id: UserId,
) -> anyhow::Result<()> {
    Ok(msg
        .edit(
            http,
            EditMessage::new().components(vec![CreateActionRow::Buttons(vec![
                CreateButton::new(build_id(first_id, user_id))
                    .style(ButtonStyle::Danger)
                    .label("Cancel"),
            ])]),
        )
        .await?)
}
