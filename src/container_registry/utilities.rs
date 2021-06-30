use crate::cmd;
use crate::container_registry::Kind;
use crate::error::{SimpleError, SimpleErrorKind};
use chrono::Duration;
use retry::delay::{Fibonacci, Range};
use retry::Error::Operation;
use retry::OperationResult;

pub fn docker_tag_and_push_image(
    container_registry_kind: Kind,
    docker_envs: Vec<(&str, &str)>,
    image_name: String,
    image_tag: String,
    dest: String,
) -> Result<(), SimpleError> {
    let image_with_tag = format!("{}:{}", image_name, image_tag);
    let registry_provider = match container_registry_kind {
        Kind::DockerHub => "DockerHub",
        Kind::Ecr => "AWS ECR",
        Kind::Docr => "DigitalOcean Registry",
    };

    match retry::retry(Fibonacci::from_millis(3000).take(5), || {
        match cmd::utilities::exec("docker", vec!["tag", &image_with_tag, dest.as_str()], &docker_envs) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => {
                info!("failed to tag image {}, retrying...", image_with_tag);
                OperationResult::Retry(e)
            }
        }
    }) {
        Err(Operation { error, .. }) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(format!("failed to tag image {}: {:?}", image_with_tag, error.message)),
            ))
        }
        _ => {}
    }

    // Note: use random time for retry instead of Fibonacci avoiding to have all parallel retries at the same time.
    // This should help reducing peak QPS and stress on the container registry.
    match retry::retry(Range::from_millis_inclusive(10, 30000).take(10), || {
        match cmd::utilities::exec_with_envs_and_output(
            "docker",
            vec!["push", dest.as_str()],
            docker_envs.clone(),
            |line| {
                let line_string = line.unwrap_or_default();
                info!("{}", line_string.as_str());
            },
            |line| {
                let line_string = line.unwrap_or_default();
                error!("{}", line_string.as_str());
            },
            Duration::minutes(10),
        ) {
            Ok(_) => OperationResult::Ok(()),
            Err(e) => {
                warn!(
                    "failed to push image {} on {}, {:?} retrying...",
                    image_with_tag, registry_provider, e.message
                );
                OperationResult::Retry(e)
            }
        }
    }) {
        Err(Operation { error, .. }) => Err(error),
        Err(e) => Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(format!(
                "unknown error while trying to push image {} to {}. {:?}",
                image_with_tag, registry_provider, e
            )),
        )),
        _ => {
            info!("image {} has successfully been pushed", image_with_tag);
            Ok(())
        }
    }
}
