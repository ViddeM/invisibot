use std::collections::{HashMap, HashSet};

use invisibot_client_api::{
    game_message::{GameMessage, GameRound, SpectatorRound},
    round_response::RoundResponse,
};
use invisibot_common::{game_error::GameResult, player_id::PlayerId, GameId};

use crate::{
    client_handler::ClientHandler, game_config::GameConfig, game_logic::game_state::GameState,
    game_map::player::Player, persistence::PersistenceHandler,
};

/// A particular game.
pub struct Game<C: ClientHandler, P: PersistenceHandler> {
    game_id: GameId,
    config: GameConfig,
    initial_state: GameState,
    game_rounds: Vec<GameState>,
    client_handler: C,
    persistence_handler: P,
}

impl<C: ClientHandler, P: PersistenceHandler> Game<C, P> {
    /// Create a new game with the `client_handler` for communicating with clients and the settings specified in `game_config`.
    pub async fn new(
        client_handler: C,
        persistence_handler: P,
        game_id: GameId,
        player_ids: HashSet<PlayerId>,
    ) -> GameResult<Self> {
        let game_config = persistence_handler.get_game_config(game_id.clone()).await?;

        let initial_game_state = GameState::new(player_ids, &game_config.map_dir)?;

        persistence_handler
            .set_game_map(game_id.clone(), initial_game_state.map.clone())
            .await?;

        Ok(Self {
            game_id,
            initial_state: initial_game_state,
            game_rounds: vec![],
            config: game_config,
            client_handler,
            persistence_handler,
        })
    }

    /// Simulate the entire game, mutating itself and storing all the states, use `get_game_rounds` to retrieve the states generated.
    pub async fn run_game(&mut self) -> GameResult<()> {
        let mut states = vec![self.initial_state.clone()];
        let mut state: GameState = self.initial_state.clone();

        for round_number in 0..(self.config.num_rounds - 1) {
            state.players.iter().for_each(|(id, _)| {
                self.client_handler.send_message(
                    id,
                    GameMessage::GameRound(GameRound::new(
                        state.map.width,
                        state.map.height,
                        state.map.get_wall_coords(),
                        state
                            .players
                            .iter()
                            .filter(|&(id, p)| id == id || p.is_visible())
                            .map(|(id, p)| (id.clone(), p.get_pos().clone()))
                            .collect(),
                        id.clone(),
                    )),
                );
            });
            self.client_handler
                .broadcast_spectators(GameMessage::GameRoundSpectators(SpectatorRound::new(
                    state.map.width,
                    state.map.height,
                    state.map.get_wall_coords(),
                    state
                        .players
                        .iter()
                        .filter(|&(id, p)| id == id || p.is_visible())
                        .map(|(id, p)| (id.clone(), p.get_pos().clone()))
                        .collect(),
                )));

            let actions: HashMap<PlayerId, RoundResponse> = self.client_handler.receive_messages();

            let (new_state, dead_players) = state.run_round(actions)?;
            dead_players.into_iter().for_each(|id: u32| {
                self.client_handler
                    .broadcast_message(GameMessage::PlayerKilled(id));
                self.client_handler
                    .broadcast_spectators(GameMessage::PlayerKilled(id));
                self.client_handler.disconnect_player(&id);
            });

            self.persistence_handler
                .save_round(
                    self.game_id,
                    round_number as u32,
                    state.players.iter().map(|(_, p)| p.clone()).collect(),
                    state.shot_tiles.iter().map(|t| t.clone()).collect(),
                )
                .await?;

            states.push(state);
            state = new_state;

            match state.players.len() {
                0 => {
                    // Nobody won
                    break;
                }
                1 => {
                    // We have a winner!
                    let winning_player = state
                        .players
                        .values()
                        .collect::<Vec<&Player>>()
                        .first()
                        .unwrap() // We just validated that there were a player left.
                        .get_id()
                        .clone();

                    self.client_handler
                        .send_message(&winning_player, GameMessage::PlayerWon(winning_player));
                    self.client_handler
                        .broadcast_spectators(GameMessage::PlayerWon(winning_player));

                    break;
                }
                _ => { /* Game's still going */ }
            }
        }

        self.client_handler
            .broadcast_message(GameMessage::ClientGoodbye("Bye".to_string()));
        self.client_handler.close();
        self.game_rounds = states;

        self.persistence_handler.game_done(self.game_id).await?;

        Ok(())
    }

    /// Get the states that occurred during the game, will be empty if `run_game` is not called first.
    pub fn get_game_rounds(&self) -> Vec<GameState> {
        self.game_rounds.clone()
    }

    /// Get the id for this game.
    pub fn get_id(&self) -> GameId {
        self.game_id.clone()
    }

    /// Cleanup after this game.
    pub fn cleanup(&mut self) {
        self.client_handler.close();
    }
}
