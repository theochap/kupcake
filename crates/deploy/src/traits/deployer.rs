//! Recursive deployer chain type.

use serde::{Deserialize, Serialize};

use super::service::KupcakeService;
use super::stages::NextStage;

/// Terminal marker for the end of a deployer chain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct End;

/// A deployer node in the deployment chain.
///
/// `S` is the service to deploy, `Next` is the rest of the chain.
/// The chain encodes deployment order through the type system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "S: KupcakeService, Next: Serialize + serde::de::DeserializeOwned")]
pub struct Deployer<S, Next = End>
where
    S: KupcakeService,
{
    /// The service configuration for this stage.
    pub service: S,
    /// The rest of the deployment chain.
    pub next: Next,
}

impl<S> Deployer<S, End>
where
    S: KupcakeService,
{
    /// Create a new deployer with a single service.
    pub fn new(service: S) -> Self {
        Self {
            service,
            next: End,
        }
    }
}

// Implement then() directly on Deployer<S, End>
impl<S> Deployer<S, End>
where
    S: KupcakeService,
{
    /// Chain another service after this one.
    ///
    /// The next service must belong to a valid subsequent stage.
    pub fn then<S2>(self, service: S2) -> Deployer<S, Deployer<S2, End>>
    where
        S2: KupcakeService,
        S::Stage: NextStage<Next = S2::Stage>,
    {
        Deployer {
            service: self.service,
            next: Deployer::new(service),
        }
    }
}

// Implement then() on Deployer<S, Deployer<S2, End>> - for 2-service chains
impl<S, S2> Deployer<S, Deployer<S2, End>>
where
    S: KupcakeService,
    S2: KupcakeService,
{
    /// Chain another service after this one.
    pub fn then<S3>(self, service: S3) -> Deployer<S, Deployer<S2, Deployer<S3, End>>>
    where
        S3: KupcakeService,
        S2::Stage: NextStage<Next = S3::Stage>,
    {
        Deployer {
            service: self.service,
            next: Deployer {
                service: self.next.service,
                next: Deployer::new(service),
                },
        }
    }
}

// Implement then() on Deployer<S, Deployer<S2, Deployer<S3, End>>> - for 3-service chains
impl<S, S2, S3> Deployer<S, Deployer<S2, Deployer<S3, End>>>
where
    S: KupcakeService,
    S2: KupcakeService,
    S3: KupcakeService,
{
    /// Chain another service after this one.
    pub fn then<S4>(self, service: S4) -> Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, End>>>>
    where
        S4: KupcakeService,
        S3::Stage: NextStage<Next = S4::Stage>,
    {
        Deployer {
            service: self.service,
            next: Deployer {
                service: self.next.service,
                next: Deployer {
                    service: self.next.next.service,
                    next: Deployer::new(service),
                },
            },
        }
    }
}

// Implement then() on Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, End>>>> - for 4-service chains
impl<S, S2, S3, S4> Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, End>>>>
where
    S: KupcakeService,
    S2: KupcakeService,
    S3: KupcakeService,
    S4: KupcakeService,
{
    /// Chain another service after this one.
    pub fn then<S5>(
        self,
        service: S5,
    ) -> Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, End>>>>>
    where
        S5: KupcakeService,
        S4::Stage: NextStage<Next = S5::Stage>,
    {
        Deployer {
            service: self.service,
            next: Deployer {
                service: self.next.service,
                next: Deployer {
                    service: self.next.next.service,
                    next: Deployer {
                        service: self.next.next.next.service,
                        next: Deployer::new(service),
                    },
                },
            },
        }
    }
}

// Implement then() on Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, End>>>>> - for 5-service chains
impl<S, S2, S3, S4, S5> Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, End>>>>>
where
    S: KupcakeService,
    S2: KupcakeService,
    S3: KupcakeService,
    S4: KupcakeService,
    S5: KupcakeService,
{
    /// Chain another service after this one.
    pub fn then<S6>(
        self,
        service: S6,
    ) -> Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, Deployer<S6, End>>>>>>
    where
        S6: KupcakeService,
        S5::Stage: NextStage<Next = S6::Stage>,
    {
        Deployer {
            service: self.service,
            next: Deployer {
                service: self.next.service,
                next: Deployer {
                    service: self.next.next.service,
                    next: Deployer {
                        service: self.next.next.next.service,
                        next: Deployer {
                            service: self.next.next.next.next.service,
                            next: Deployer::new(service),
                        },
                    },
                },
            },
        }
    }
}

// Implement then() on Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, Deployer<S6, End>>>>>> - for 6-service chains
impl<S, S2, S3, S4, S5, S6>
    Deployer<S, Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, Deployer<S6, End>>>>>>
where
    S: KupcakeService,
    S2: KupcakeService,
    S3: KupcakeService,
    S4: KupcakeService,
    S5: KupcakeService,
    S6: KupcakeService,
{
    /// Chain another service after this one.
    pub fn then<S7>(
        self,
        service: S7,
    ) -> Deployer<
        S,
        Deployer<S2, Deployer<S3, Deployer<S4, Deployer<S5, Deployer<S6, Deployer<S7, End>>>>>>,
    >
    where
        S7: KupcakeService,
        S6::Stage: NextStage<Next = S7::Stage>,
    {
        Deployer {
            service: self.service,
            next: Deployer {
                service: self.next.service,
                next: Deployer {
                    service: self.next.next.service,
                    next: Deployer {
                        service: self.next.next.next.service,
                        next: Deployer {
                            service: self.next.next.next.next.service,
                            next: Deployer {
                                service: self.next.next.next.next.next.service,
                                next: Deployer::new(service),
                            },
                        },
                    },
                },
            },
        }
    }
}
