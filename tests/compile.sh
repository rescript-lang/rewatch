echo "Test: It should compile"
cd ../testrepo

if RUST_BACKTRACE=1 ../target/release/rewatch clean .;
then
  echo "✅ - Repo Cleaned"
else 
  echo "❌ - Error Cleaning Repo"
  exit 1
fi

if RUST_BACKTRACE=1 ../target/release/rewatch build .; 
then
  echo "✅ - Repo Built"
else 
  echo "❌ - Error Building Repo"
  exit 1
fi


if git diff --exit-code ./; 
then
  echo "✅ - Testrepo has no changes"
else 
  echo "❌ - Build has changed"
  exit 1
fi

node ./packages/main/src/Main.mjs > ./packages/main/src/output.txt

if git diff --exit-code ./; 
then
  echo "✅ - Output is correct"
else 
  echo "❌ - Output is incorrect"
  exit 1
fi
