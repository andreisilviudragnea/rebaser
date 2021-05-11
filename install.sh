sdk install java 16.0.1-zulu
sdk use java 16.0.1-zulu

./gradlew clean build

java -jar "$(pwd)/build/libs/rebaser-1.0-SNAPSHOT.jar"
