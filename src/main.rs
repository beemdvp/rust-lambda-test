/// Assumes a you are running the following `dynamodb-local`
/// on your host machine
///
/// ```bash
/// $ docker run -p 8000:8000 amazon/dynamodb-local
/// ```
use std::{collections::HashMap, env};

use dynomite::{
    dynamodb::{DynamoDb, DynamoDbClient, GetItemInput},
    retry::Policy,
    retry::RetryingDynamoDb,
    AttributeError, FromAttributes, Item, Retries,
};
use lambda_http::{
    handler, http::StatusCode, lambda, Context, IntoResponse, Request, RequestExt, Response,
};
use rusoto_core::Region;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[macro_use]
extern crate lazy_static;
#[macro_use]
#[cfg(not(test))]
extern crate log;

lazy_static! {
    static ref DB: RetryingDynamoDb<DynamoDbClient> = create_client();
}

#[derive(Item, Debug, Clone, Serialize, Deserialize)]
pub struct BookEntity {
    #[dynomite(partition_key)]
    #[serde(with = "json_uuid")]
    id: Uuid,
    #[dynomite(rename = "bookTitle", default)]
    title: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    RequestInvalid,
    RequestUnauthorized,
    NotFound,
    InternalServerError,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    request_id: String,
    error_type: ErrorType,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_codes: Option<Vec<String>>,
}

impl ErrorResponse {
    pub fn not_found(request_id: String) -> Self {
        Self {
            request_id,
            error_type: ErrorType::NotFound,
            error_codes: None,
        }
    }

    pub fn internal_server(request_id: String) -> Self {
        Self {
            request_id,
            error_type: ErrorType::InternalServerError,
            error_codes: None,
        }
    }
}

mod json_uuid {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(id.to_string().as_str())
    }

    pub fn deserialize<'de, S>(deserializer: S) -> Result<Uuid, S::Error>
    where
        S: Deserializer<'de>,
    {
        let uuid = String::deserialize(deserializer)?;
        Ok::<_, S::Error>(Uuid::parse_str(uuid.as_str()).unwrap())
    }
}

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

pub fn create_client() -> RetryingDynamoDb<DynamoDbClient> {
    let local_client = DynamoDbClient::new(Region::Custom {
        name: "us-east-1".into(),
        endpoint: "http://localhost:8000".into(),
    })
    .with_retries(Policy::default());

    let remote_client = DynamoDbClient::new(Region::EuWest2).with_retries(Policy::default());

    let env = env::var("ENV").unwrap_or("".into());

    match env == "live" {
        true => remote_client,
        _ => local_client
    }
}

pub fn not_found(request_id: String) -> Response<String> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(serde_json::to_string(&ErrorResponse::not_found(request_id)).unwrap())
        .unwrap()
}

fn internal_server(request_id: String) -> Response<String> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(serde_json::to_string(&ErrorResponse::internal_server(request_id)).unwrap())
        .unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();
    lambda::run(handler(hello)).await?;
    Ok(())
}

async fn hello(request: Request, context: Context) -> Result<impl IntoResponse, Error> {
    let qs = request.path_parameters();
    let key = qs.get("id").or_else(|| Some("")).unwrap();

    info!("main: get by id: {:?}", key);

    let attr = dynomite::AttributeValue {
        s: Some(key.to_string()),
        ..dynomite::AttributeValue::default()
    };

    let mut map = HashMap::new();
    map.insert("id".to_string(), attr);
    let book_raw_item = DB
        .get_item(GetItemInput {
            table_name: "books".to_string(),
            key: map,
            ..GetItemInput::default()
        })
        .await;

    match book_raw_item {
        Err(e) => {
            info!("dynamodb failed: {:?}", e);
            Ok(internal_server(context.request_id))
        }
        Ok(b) => {
            info!("main: fetched book, found {:?}", b);

            let try_book: Option<Result<BookEntity, AttributeError>> =
                b.item.map(BookEntity::from_attrs);

            if try_book.is_some() {
                let book_result = try_book.unwrap();
                let books: BookEntity = book_result.expect("result no work");
                info!("main: parsing to entity {:?}", books);

                let r = Response::builder()
                    .status(200)
                    .header("x-foo-bar", "bar")
                    .header("x-bar-baz", "baz")
                    .body(serde_json::to_string(&books).unwrap())
                    .unwrap();
                Ok::<_, Error>(r)
            } else {
                Ok(not_found(context.request_id))
            }
        }
    }
}

#[cfg(test)]
use std::println as info;

#[cfg(test)]
mod tests {
    use dynomite::AttributeValue;
    use lambda_http::{http::Request, Body, Context, IntoResponse, StrMap};
    use rusoto_core::RusotoError;
    use rusoto_dynamodb::{
        DeleteItemError, DeleteItemInput, DeleteItemOutput, PutItemError, PutItemInput,
        PutItemOutput,
    };

    use super::*;

    pub async fn insert_book(
        client: &RetryingDynamoDb<DynamoDbClient>,
        book: &BookEntity,
    ) -> Result<PutItemOutput, RusotoError<PutItemError>> {
        client
            .put_item(PutItemInput {
                table_name: "books".to_string(),
                item: book.clone().into(),
                ..PutItemInput::default()
            })
            .await
    }

    pub async fn delete_book(
        client: &RetryingDynamoDb<DynamoDbClient>,
        key: HashMap<String, AttributeValue>,
    ) -> Result<DeleteItemOutput, RusotoError<DeleteItemError>> {
        client
            .delete_item(DeleteItemInput {
                table_name: "books".to_string(),
                key,
                ..DeleteItemInput::default()
            })
            .await
    }

    pub fn get_body<'a, T>(body: &'a Body) -> T
    where
        T: Deserialize<'a>,
    {
        let text = match body {
            Body::Text(e) => e.as_str(),
            _ => "",
        };

        serde_json::from_str(text).unwrap()
    }

    #[tokio::test]
    async fn hello_handles() {
        let mut hash = HashMap::new();

        let rust_book = BookEntity {
            id: Uuid::new_v4(),
            title: "rust".into(),
        };

        insert_book(&DB, &rust_book).await.unwrap();

        hash.insert("id".to_string(), vec![rust_book.id.to_string()]);
        let request = Request::<Body>::default().with_path_parameters(StrMap::from(hash));

        let response = hello(request, Context::default())
            .await
            .expect("Did not work")
            .into_response();

        let b: BookEntity = get_body(&response.body());

        assert_eq!(b.title, "rust");
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("x-foo-bar").unwrap(), "bar");
        assert_eq!(response.headers().get("x-bar-baz").unwrap(), "baz");
        delete_book(&DB, rust_book.key()).await.unwrap();
    }

    #[tokio::test]
    async fn hello_handles_not_found() {
        let mut hash = HashMap::new();

        hash.insert("id".to_string(), vec!["foo-bar".to_string()]);
        let request = Request::<Body>::default().with_path_parameters(StrMap::from(hash));

        let response = hello(request, Context::default())
            .await
            .expect("Did not work")
            .into_response();

        let b: ErrorResponse = get_body(&response.body());

        assert_eq!(b.error_type, ErrorType::NotFound);
        assert_eq!(b.error_codes, None);
        assert_eq!(response.status(), 404);
    }
}
