//! Redis support for the `bb8` connection pool.
//!
//! # Examples
//!
//! ```rust,no_run
//! use bb8_redis::*;
//! use redis::{RedisError, Script, Commands, Client};
//! use futures::Future;
//!
//! let client = Client::open("redis://127.0.0.1/").unwrap();
//!
//! let manager = RedisConnectionManager::new(client).unwrap();
//!
//! let redis_pool = bb8::Builder::default()
//!     .max_size(4)
//!     .build(manager)
//!     .map(RedisPool::new)
//!     .map_err(|_err| bb8::RunError::TimedOut);
//!
//! let app = redis_pool
//!     .and_then(|pool| {
//!         pool.run(|mut conn| {
//!             redis::cmd("sadd").arg("key").arg("value").query_async::<_, usize>(conn)
//!         })
//!     });
//!
//! // run app future
//!
//! ```
#![deny(missing_docs, missing_debug_implementations)]

pub use bb8;
pub use redis;

use futures::{Future, IntoFuture};

use redis::aio::SharedConnection;
use redis::{Client, RedisError};

use std::option::Option;

type Result<T> = std::result::Result<T, RedisError>;

/// `RedisPool` is a convenience wrapper around `bb8::Pool` that hides the fact that
/// `RedisConnectionManager` uses an `Option<Connection>` to smooth over the API incompatibility.
#[derive(Debug, Clone)]
pub struct RedisPool {
    pool: bb8::Pool<RedisConnectionManager>,
}

impl RedisPool {
    /// Constructs a new `RedisPool`, see the `bb8::Builder` documentation for description of
    /// parameters.
    pub fn new(pool: bb8::Pool<RedisConnectionManager>) -> RedisPool {
        RedisPool { pool }
    }

    /// Run the function with a connection provided by the pool.
    pub fn run<'a, T, E, U, F>(
        &self,
        f: F,
    ) -> impl Future<Item = T, Error = bb8::RunError<E>> + Send + 'a
    where
        F: FnOnce(SharedConnection) -> U + Send + 'a,
        U: IntoFuture<Item = (SharedConnection, T), Error = E> + 'a,
        U::Future: Send,
        E: From<<RedisConnectionManager as bb8::ManageConnection>::Error> + Send + 'a,
        T: Send + 'a,
    {
        let f = move |conn: Option<SharedConnection>| {
            let conn = conn.unwrap();
            f(conn)
                .into_future()
                .map(|(conn, item)| (item, Some(conn)))
                .map_err(|err| (err, None))
        };
        self.pool.run(f)
    }
}

/// A `bb8::ManageConnection` for `redis::async::Connection`s.
#[derive(Clone, Debug)]
pub struct RedisConnectionManager {
    client: Client,
}

impl RedisConnectionManager {
    /// Create a new `RedisConnectionManager`.
    pub fn new(client: Client) -> Result<RedisConnectionManager> {
        Ok(RedisConnectionManager { client })
    }
}

impl bb8::ManageConnection for RedisConnectionManager {
    type Connection = Option<SharedConnection>;
    type Error = RedisError;

    fn connect(
        &self,
    ) -> Box<dyn Future<Item = Self::Connection, Error = Self::Error> + Send + 'static> {
        Box::new(
            self.client
                .get_shared_async_connection()
                .map(|conn| Some(conn)),
        )
    }

    fn is_valid(
        &self,
        conn: Self::Connection,
    ) -> Box<dyn Future<Item = Self::Connection, Error = (Self::Error, Self::Connection)> + Send>
    {
        // The connection should only be None after a failure.
        Box::new(
            redis::cmd("PING")
                .query_async(conn.unwrap())
                .map_err(|err| (err, None))
                .map(|(conn, ())| Some(conn)),
        )
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        conn.is_none()
    }
}
