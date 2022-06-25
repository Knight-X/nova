use run_script::ScriptOptions;

pub fn run_dl()  {
    let options = ScriptOptions::new();

    let args = vec![];

    // run the script and get the script execution output
    let (code, output, error) = run_script::run(
        r#"
         echo "Directory Info:"
         cd ../jax/
         dir
        pipenv run python3 ml_service.py
         "#,
        &args,
        &options,
    )
    .unwrap();

    println!("Exit Code: {}", code);
    println!("Output: {}", output);
    println!("I am warning: {}", error);
}

