use std::{fmt, hash::Hash};

use crate::sinks::prelude::*;

use super::partitioner::S3PartitionKey;
use vector_lib::{event::Event, partition::Partitioner};

pub struct S3Sink<Svc, RB, P> {
    service: Svc,
    request_builder: RB,
    partitioner: P,
    batcher_settings: BatcherSettings,
}

impl<Svc, RB, P> S3Sink<Svc, RB, P> {
    pub const fn new(
        service: Svc,
        request_builder: RB,
        partitioner: P,
        batcher_settings: BatcherSettings,
    ) -> Self {
        Self {
            partitioner,
            service,
            request_builder,
            batcher_settings,
        }
    }
}

impl<Svc, RB, P> S3Sink<Svc, RB, P>
where
    Svc: Service<RB::Request> + Send + 'static,
    Svc::Future: Send + 'static,
    Svc::Response: DriverResponse + Send + 'static,
    Svc::Error: fmt::Debug + Into<crate::Error> + Send,
    RB: RequestBuilder<(S3PartitionKey, Vec<Event>)> + Send + Sync + 'static,
    RB::Error: fmt::Display + Send,
    RB::Request: Finalizable + MetaDescriptive + Send,
    P: Partitioner<Item = Event, Key = Option<S3PartitionKey>> + Unpin + Send,
    P::Key: Eq + Hash + Clone,
    P::Item: ByteSizeOf,
{
    async fn run_inner(self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        let partitioner = self.partitioner;
        let settings = self.batcher_settings;
        let request_builder = self.request_builder;

        input
            .batched_partitioned(partitioner, || settings.as_byte_size_config())
            .filter_map(|(key, batch)| async move { key.map(move |k| (k, batch)) })
            .request_builder(default_request_builder_concurrency_limit(), request_builder)
            .filter_map(|request| async move {
                match request {
                    Err(error) => {
                        emit!(SinkRequestBuildError { error });
                        None
                    }
                    Ok(req) => Some(req),
                }
            })
            .into_driver(self.service)
            .run()
            .await
    }
}

#[async_trait]
impl<Svc, RB, P> StreamSink<Event> for S3Sink<Svc, RB, P>
where
    Svc: Service<RB::Request> + Send + 'static,
    Svc::Future: Send + 'static,
    Svc::Response: DriverResponse + Send + 'static,
    Svc::Error: fmt::Debug + Into<crate::Error> + Send,
    RB: RequestBuilder<(S3PartitionKey, Vec<Event>)> + Send + Sync + 'static,
    RB::Error: fmt::Display + Send,
    RB::Request: Finalizable + MetaDescriptive + Send,
    P: Partitioner<Item = Event, Key = Option<S3PartitionKey>> + Unpin + Send,
    P::Key: Eq + Hash + Clone,
    P::Item: ByteSizeOf,
{
    async fn run(mut self: Box<Self>, input: BoxStream<'_, Event>) -> Result<(), ()> {
        self.run_inner(input).await
    }
}
