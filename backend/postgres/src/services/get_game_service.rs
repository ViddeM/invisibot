use invisibot_game::{
    clients::player_id::PlayerId,
    coord,
    game_config::GameConfig,
    game_map::game_map::GameMap,
    persistence::{
        completed_game::{CompletedGame, GameRound, RoundPlayer},
        GameId,
    },
    utils::{coordinate::Coordinate, direction::Direction},
};

use crate::{
    db_connection::DBConnection,
    postgres_error::PostgresResult,
    repositories::{
        game_repository, map_repository, map_wall_repository, player_repository, round_repository,
        shot_tile_repository, starting_position_repository,
    },
    tables::{game::Game, player::Player},
};

pub async fn get_game(conn: &DBConnection, game_id: GameId) -> PostgresResult<Option<Game>> {
    let mut transaction = conn.new_transaction().await?;
    let game = game_repository::try_get_game_by_id(&mut transaction, game_id).await?;
    transaction.commit().await?;
    Ok(game)
}

pub async fn get_finished_game(
    conn: &DBConnection,
    game_id: GameId,
) -> PostgresResult<CompletedGame> {
    // TODO: Check if the game is actually finished.

    let mut transaction = conn.new_transaction().await?;

    let map = map_repository::get_map_by_game_id(&mut transaction, game_id).await?;
    let map_walls = map_wall_repository::get_by_map_id(&mut transaction, map.id).await?;
    let starting_positions =
        starting_position_repository::get_by_map_id(&mut transaction, map.id).await?;

    let game_map = GameMap::new(
        map.width as u32,
        map.height as u32,
        starting_positions
            .into_iter()
            .map(|s| coord!(s.x as u32, s.y as u32))
            .collect(),
        map_walls
            .into_iter()
            .map(|w| coord!(w.x as u32, w.y as u32))
            .collect(),
    );

    let game_rounds = round_repository::get_rounds_by_game_id(&mut transaction, game_id).await?;

    let mut rounds = Vec::new();

    for round in game_rounds.into_iter() {
        let players = player_repository::get_players_by_game_and_round(
            &mut transaction,
            game_id,
            round.round_number,
        )
        .await?
        .into_iter()
        .map(|p| player_to_round_player(p))
        .collect::<PostgresResult<Vec<RoundPlayer>>>()?;

        let shot_tiles = shot_tile_repository::get_by_game_and_round(
            &mut transaction,
            game_id,
            round.round_number,
        )
        .await?
        .into_iter()
        .map(|t| coord!(t.x as u32, t.y as u32))
        .collect();

        rounds.push(GameRound {
            round_number: round.round_number as u32,
            players,
            shot_tiles,
        });
    }

    transaction.commit().await?;
    Ok(CompletedGame {
        game_id,
        map: game_map,
        rounds: rounds,
    })
}

fn player_to_round_player(player: Player) -> PostgresResult<RoundPlayer> {
    Ok(RoundPlayer {
        id: player.id as PlayerId,
        position: coord!(player.x as u32, player.y as u32),
        rotation: Direction::try_from(player.rotation)?,
        visible: player.visible,
    })
}

pub async fn get_game_num_players(conn: &DBConnection, game_id: GameId) -> PostgresResult<u32> {
    let mut transaction = conn.new_transaction().await?;
    let game = game_repository::get_game_by_id(&mut transaction, game_id).await?;
    Ok(game.num_players as u32)
}

pub async fn get_game_config(conn: &DBConnection, game_id: GameId) -> PostgresResult<GameConfig> {
    let mut transaction = conn.new_transaction().await?;
    let game = game_repository::get_game_by_id(&mut transaction, game_id).await?;
    Ok(GameConfig {
        num_players: game.num_players as usize,
        num_rounds: game.max_num_rounds as usize,
        map_dir: game.map_dir,
    })
}
