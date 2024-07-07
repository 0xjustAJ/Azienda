#[macro_use]
extern crate serde;
use candid::{Decode, Encode};
use ic_cdk::api::time;
use ic_stable_structures::memory_manager::{self, MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};

/* Defining Memory state and IdCell */
type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

/* Defining the job application struct */
#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Job {
    id: u64, // the id for the job
    title: String, // the job title
    description: String, // job description
    created_at: u64,
    applicant_name: Vec<String>,
    accepted_applicants: Option<String>,
}

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct CreateJob {
    title: String,
    description: String,
}

/* Enumeration for the error */
#[derive(candid::CandidType, Deserialize, Serialize)]
enum Error {
    JobNotFound { msg: String },
    InvalidInput { msg: String },
}

/* Job status enumeration */
#[derive(candid::CandidType, Deserialize, Serialize)]
enum JobStatus {
    AcceptJob,
    JobWithdrawn,
    JobCancelled,
}

/* Implementing the Storable trait for Job struct */
impl Storable for Job {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 1024,
        is_fixed_size: false,
    };
}

/* Thread-local storage setup */
thread_local! {
    /* Memory manager for the canister */
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    /* ID counter for the canister */
    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0).expect("cannot create counter")
    );

    /* Storage for the canister */
    static STORAGE: RefCell<StableBTreeMap<u64, Job, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
        ));
}

/* Function to create a job */
#[ic_cdk::update]
fn create_job(job: CreateJob) -> Result<Job, Error> {
    // Validate input payload
    if job.title.is_empty() || job.description.is_empty() {
        return Err(Error::InvalidInput {
            msg: "All fields must be provided and non-empty".to_string(),
        });
    }

    // Increment the ID counter
    let id = ID_COUNTER.with(|counter| {
        let current_value = *counter.borrow().get();
        counter.borrow_mut().set(current_value + 1).expect("Cannot increment id counter");
        current_value + 1
    });

    // Create a new Job struct
    let job = Job {
        id,
        title: job.title,
        description: job.description,
        created_at: time(),
        applicant_name: vec![],
        accepted_applicants: None,
    };

    // Insert the new job into storage
    STORAGE.with(|storage| storage.borrow_mut().insert(job.id, job.clone()));
    Ok(job)
}

/* Function to apply for a job */
#[ic_cdk::update]
fn apply_to_job(job_id: u64, applicant_name: String) -> Result<(), Error> {
    // Validate input payload
    if applicant_name.is_empty() {
        return Err(Error::InvalidInput {
            msg: "Applicant name must be provided and non-empty".to_string(),
        });
    }

    STORAGE.with(|storage| {
        let mut job_opt = {
            let mut storage_ref = storage.borrow_mut();
            storage_ref.get(&job_id).clone()
        };

        if let Some(mut job) = job_opt {
            job.applicant_name.push(applicant_name);

            STORAGE.with(|storage| {
                storage.borrow_mut().insert(job.id, job);
            });

            Ok(())
        } else {
            Err(Error::JobNotFound {
                msg: format!("Job with id {} not found", job_id),
            })
        }
    })
}

/* Function to withdraw an application */
#[ic_cdk::update]
fn withdraw_application(job_id: u64, applicant_name: String) -> Result<(), Error> {
    // Validate input payload
    if applicant_name.is_empty() {
        return Err(Error::InvalidInput {
            msg: "Applicant name must be provided and non-empty".to_string(),
        });
    }

    let mut job_opt = STORAGE.with(|storage| {
        storage.borrow().get(&job_id).clone()
    });

    if let Some(mut job) = job_opt {
        job.applicant_name.retain(|name| name != &applicant_name);

        STORAGE.with(|storage| {
            storage.borrow_mut().insert(job.id, job);
        });

        Ok(())
    } else {
        Err(Error::JobNotFound {
            msg: format!("Job with id {} not found", job_id),
        })
    }
}

/* Function to cancel a job */
#[ic_cdk::update]
fn cancel_job(job_id: u64) -> Result<(), Error> {
    STORAGE.with(|storage| {
        if storage.borrow_mut().remove(&job_id).is_some() {
            Ok(())
        } else {
            Err(Error::JobNotFound {
                msg: format!("Job with id {} not found", job_id),
            })
        }
    })
}

/* Function to accept a job application */
#[ic_cdk::update]
fn accept_job(job_id: u64, applicant_name: String) -> Result<(), Error> {
    // Validate input payload
    if applicant_name.is_empty() {
        return Err(Error::InvalidInput {
            msg: "Applicant name must be provided and non-empty".to_string(),
        });
    }

    let mut job_opt = STORAGE.with(|storage| {
        storage.borrow().get(&job_id).clone()
    });

    if let Some(mut job) = job_opt {
        if job.applicant_name.contains(&applicant_name) {
            job.accepted_applicants = Some(applicant_name);

            STORAGE.with(|storage| {
                storage.borrow_mut().insert(job.id, job);
            });

            Ok(())
        } else {
            Err(Error::InvalidInput {
                msg: format!("Applicant {} not found in job {}", applicant_name, job_id),
            })
        }
    } else {
        Err(Error::JobNotFound {
            msg: format!("Job with id {} not found", job_id),
        })
    }
}

/* Function to fetch a job by ID */
#[ic_cdk::query]
fn fetch_job(job_id: u64) -> Result<Job, Error> {
    STORAGE.with(|storage| {
        if let Some(job) = storage.borrow().get(&job_id) {
            Ok(job.clone())
        } else {
            Err(Error::JobNotFound {
                msg: format!("Job with id {} not found", job_id),
            })
        }
    })
}

ic_cdk::export_candid!();
