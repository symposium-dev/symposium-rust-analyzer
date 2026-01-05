use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Person {
    name: String,
    age: u32,
    email: Option<String>,
}

impl Person {
    pub fn new(name: String, age: u32) -> Self {
        Self {
            name,
            age,
            email: None,
        }
    }

    pub fn set_email(&mut self, email: String) {
        self.email = Some(email);
    }

    pub fn greet(&self) -> String {
        format!("Hello, my name is {}", self.name)
    }
}

pub trait Displayable {
    fn display(&self) -> String;
}

impl Displayable for Person {
    fn display(&self) -> String {
        match &self.email {
            Some(email) => format!("{} ({})", self.name, email),
            None => self.name.clone(),
        }
    }
}

pub fn calculate_sum(numbers: &[i32]) -> i32 {
    numbers.iter().sum()
}

pub fn find_max<T: PartialOrd + Copy>(items: &[T]) -> Option<T> {
    items.iter().max().copied()
}

pub enum Status {
    Active,
    Inactive,
    Pending(String),
}

pub struct Database {
    users: HashMap<u32, Person>,
    next_id: u32,
}

impl Database {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn add_user(&mut self, mut person: Person) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.users.insert(id, person);
        id
    }

    pub fn get_user(&self, id: u32) -> Option<&Person> {
        self.users.get(&id)
    }

    pub fn update_user_email(&mut self, id: u32, email: String) -> Result<(), String> {
        match self.users.get_mut(&id) {
            Some(user) => {
                user.set_email(email);
                Ok(())
            }
            None => Err("User not found".to_string()),
        }
    }
}

fn main() {
    let mut db = Database::new();
    
    let person1 = Person::new("Alice".to_string(), 30);
    let person2 = Person::new("Bob".to_string(), 25);
    
    let id1 = db.add_user(person1);
    let id2 = db.add_user(person2);
    
    if let Some(user) = db.get_user(id1) {
        println!("{}", user.greet());
        println!("Display: {}", user.display());
    }
    
    let _ = db.update_user_email(id1, "alice@example.com".to_string());
    
    let numbers = vec![1, 2, 3, 4, 5];
    let sum = calculate_sum(&numbers);
    println!("Sum: {}", sum);
    
    let max_num = find_max(&numbers);
    println!("Max: {:?}", max_num);
    
    let status = Status::Pending("Verification".to_string());
    match status {
        Status::Active => println!("User is active"),
        Status::Inactive => println!("User is inactive"),
        Status::Pending(reason) => println!("User is pending: {}", reason),
    }
    
    // This line has an intentional error for testing diagnostics
    error_function();
}

// This function doesn't exist - intentional error for testing