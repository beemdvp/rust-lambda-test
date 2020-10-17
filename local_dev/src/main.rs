use dynomite::{retry::Policy, Retries};
use rusoto_core::Region;
use rusoto_dynamodb::{
    AttributeDefinition, CreateTableInput, DynamoDb, DynamoDbClient, KeySchemaElement,
    ProvisionedThroughput,
};
use tokio::time::Error;

pub async fn bootstrap<D>(client: &D, table_name: String)
where
    D: DynamoDb,
{
    let _ = client
        .create_table(CreateTableInput {
            table_name,
            key_schema: vec![KeySchemaElement {
                attribute_name: "id".into(),
                key_type: "HASH".into(),
            }],
            attribute_definitions: vec![AttributeDefinition {
                attribute_name: "id".into(),
                attribute_type: "S".into(),
            }],
            provisioned_throughput: Some(ProvisionedThroughput {
                read_capacity_units: 1,
                write_capacity_units: 1,
            }),
            ..CreateTableInput::default()
        })
        .await;
}

#[tokio::main]
pub async fn main() -> Result<(), Error> {
    println!("Importing tables");
    let local_client = DynamoDbClient::new(Region::Custom {
        name: "us-east-1".into(),
        endpoint: "http://localhost:8000".into(),
    })
    .with_retries(Policy::default());

    bootstrap(&local_client, "books".into()).await;
    println!("Finished: Importing tables");

    Ok(())
}
